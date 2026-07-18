//! HTTP downloads. Small fetches (`entry.json`, icons) read the whole body; a big download (an
//! APK) streams in 64 KiB chunks into a caller-provided sink — a file for install, so a 500 MB
//! APK never sits in memory — reporting progress, cancellable mid-flight, and hashing as it goes.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use sha2::{Digest, Sha256};

/// Fetch a URL fully into memory. Used for `entry.json`, the index, and images.
pub fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut resp = ureq::get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("read failed: {e}"))?;
    Ok(buf)
}

/// Ordered download targets: the primary base first, then its mirrors — the failover list every
/// mirror-aware fetch tries in turn (#12).
pub fn bases(primary: &str, mirrors: &[String]) -> Vec<String> {
    let mut v = Vec::with_capacity(1 + mirrors.len());
    v.push(primary.to_string());
    v.extend(mirrors.iter().cloned());
    v
}

/// [`get_bytes`] with mirror failover: tries `{base}{path}` for each base in order, returning the
/// first success (or the last error if every host fails).
pub fn get_bytes_from(bases: &[String], path: &str) -> Result<Vec<u8>, String> {
    let mut last = "no repository URL".to_string();
    for base in bases {
        match get_bytes(&format!("{base}{path}")) {
            Ok(b) => return Ok(b),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// [`get_string`] with mirror failover.
pub fn get_string_from(bases: &[String], path: &str) -> Result<String, String> {
    let bytes = get_bytes_from(bases, path)?;
    String::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {e}"))
}

/// Shared, thread-safe progress handle for a running download.
#[derive(Clone, Default)]
pub struct Progress {
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }
    /// Bytes downloaded so far.
    pub fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }
    /// Total bytes, or 0 if the server didn't say.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
    /// Fraction in `0.0..=1.0`, or `None` when the total is unknown.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total();
        (total > 0).then(|| (self.downloaded() as f32 / total as f32).clamp(0.0, 1.0))
    }
    /// Request cancellation; the download returns `Err("cancelled")` at the next chunk.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Stream `url` into `sink`, updating `progress` as bytes arrive and folding each chunk into a
/// running SHA-256. Returns the lowercase-hex digest so a caller can verify the download against a
/// catalog hash without a second pass. Returns `Err("cancelled")` if `progress.cancel()` was
/// called, or `Err(...)` on a network / write error — the caller owns `sink` and must discard any
/// partial output (e.g. delete the temp file) in those cases. `fallback_total` seeds the size when
/// the server omits `Content-Length` (the index's `entry.json` knows the APK size).
pub fn download_to(
    url: &str,
    sink: &mut impl Write,
    progress: &Progress,
    fallback_total: u64,
) -> Result<String, String> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let total = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(fallback_total);
    progress.total.store(total, Ordering::Relaxed);
    progress.downloaded.store(0, Ordering::Relaxed);

    let mut reader = resp.into_body().into_reader();
    let mut hasher = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if progress.is_cancelled() {
            return Err("cancelled".to_string());
        }
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("read failed: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
        sink.write_all(&chunk[..n])
            .map_err(|e| format!("write failed: {e}"))?;
        progress.downloaded.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// Download `url` to `path`, **resuming** an existing partial file with an HTTP `Range` request
/// when one is present (#11). Streams in 64 KiB chunks (a large APK never sits in memory), reports
/// progress, is cancellable, and returns the whole file's lowercase-hex SHA-256 — re-hashing the
/// bytes already on disk so the digest covers the complete file. If the server ignores `Range`
/// (answers `200` instead of `206`), it restarts cleanly from zero.
pub fn download_to_file_resumable(
    url: &str,
    path: &std::path::Path,
    progress: &Progress,
    fallback_total: u64,
) -> Result<String, String> {
    let existing = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    // Ask to resume when a partial file exists; fall back to a fresh GET on any Range failure.
    let (mut reader, mut downloaded, total, append) = if existing > 0 {
        match ureq::get(url)
            .header("Range", &format!("bytes={existing}-"))
            .call()
        {
            Ok(resp) if resp.status().as_u16() == 206 => {
                // The full size is the `/total` in `Content-Range: bytes start-end/total`.
                let total = resp
                    .headers()
                    .get("Content-Range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.rsplit('/').next())
                    .and_then(|t| t.trim().parse::<u64>().ok())
                    .or_else(|| header_u64(&resp, "Content-Length").map(|l| existing + l))
                    .unwrap_or(fallback_total);
                (resp.into_body().into_reader(), existing, total, true)
            }
            // Range ignored (200) → the body is the whole file; restart from scratch.
            Ok(resp) => {
                let total = header_u64(&resp, "Content-Length").unwrap_or(fallback_total);
                (resp.into_body().into_reader(), 0, total, false)
            }
            Err(_) => {
                let resp = ureq::get(url)
                    .call()
                    .map_err(|e| format!("request failed: {e}"))?;
                let total = header_u64(&resp, "Content-Length").unwrap_or(fallback_total);
                (resp.into_body().into_reader(), 0, total, false)
            }
        }
    } else {
        let resp = ureq::get(url)
            .call()
            .map_err(|e| format!("request failed: {e}"))?;
        let total = header_u64(&resp, "Content-Length").unwrap_or(fallback_total);
        (resp.into_body().into_reader(), 0, total, false)
    };

    progress.total.store(total, Ordering::Relaxed);
    progress.downloaded.store(downloaded, Ordering::Relaxed);

    // Seed the hasher: with the on-disk bytes when appending, else start clean (and truncate).
    let mut hasher = Sha256::new();
    let mut sink = if append {
        seed_hasher_from_file(path, &mut hasher)?;
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| format!("open failed: {e}"))?
    } else {
        std::fs::File::create(path).map_err(|e| format!("create file: {e}"))?
    };

    let mut chunk = [0u8; 64 * 1024];
    loop {
        if progress.is_cancelled() {
            return Err("cancelled".to_string());
        }
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("read failed: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
        sink.write_all(&chunk[..n])
            .map_err(|e| format!("write failed: {e}"))?;
        downloaded += n as u64;
        progress.downloaded.store(downloaded, Ordering::Relaxed);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// Fold a file's bytes into `hasher` in bounded chunks (used to re-hash a resumed partial).
fn seed_hasher_from_file(path: &std::path::Path, hasher: &mut Sha256) -> Result<(), String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("reopen failed: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("rehash failed: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(())
}

/// Parse a `u64` response header, e.g. `Content-Length`.
fn header_u64<B>(resp: &ureq::http::Response<B>, name: &str) -> Option<u64> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

/// Stream `url` fully into memory (plus its SHA-256), for the bounded catalog index. Prefer
/// [`download_to`] with a file sink for large payloads like APKs.
pub fn download(
    url: &str,
    progress: &Progress,
    fallback_total: u64,
) -> Result<(Vec<u8>, String), String> {
    let mut out = Vec::with_capacity(fallback_total as usize);
    let digest = download_to(url, &mut out, progress, fallback_total)?;
    Ok((out, digest))
}

/// Lowercase-hex encoding of a byte slice (SHA-256 digests are compared as lowercase hex).
fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::bases;

    #[test]
    fn bases_put_primary_first() {
        let mirrors = vec!["https://m1/repo".to_string(), "https://m2/repo".to_string()];
        assert_eq!(
            bases("https://primary/repo", &mirrors),
            vec![
                "https://primary/repo".to_string(),
                "https://m1/repo".to_string(),
                "https://m2/repo".to_string(),
            ]
        );
        // No mirrors → just the primary.
        assert_eq!(
            bases("https://primary/repo", &[]),
            vec!["https://primary/repo".to_string()]
        );
    }
}
