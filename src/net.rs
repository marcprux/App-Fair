//! HTTP downloads via `day-part-http` (the platform HTTP stack — system proxies, VPN, and TLS on
//! macOS/iOS/Android/Windows; a bundled ureq+rustls fallback on Linux/OHOS). Small fetches
//! (`entry.json`, icons) read the whole body; a big download (an APK) streams in chunks into a
//! caller-provided sink — a file for install, so a 500 MB APK never sits in memory — reporting
//! progress, cancellable mid-flight, and hashing as it goes.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use day_part_http::{HttpError, Request, StreamSink};
use sha2::{Digest, Sha256};

/// Fetch a URL fully into memory. Used for `entry.json`, the index, and images.
pub fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp =
        day_part_http::fetch(&Request::get(url)).map_err(|e| format!("request failed: {e}"))?;
    // day-part-http hands 4xx/5xx back as responses; a miss must fail over to the next mirror.
    if !(200..300).contains(&resp.status) {
        return Err(format!("request failed: HTTP {}", resp.status));
    }
    Ok(resp.body)
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

/// Case-insensitive response-header lookup over day-part-http's header list.
fn header<'h>(headers: &'h [(String, String)], name: &str) -> Option<&'h str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Parse a `u64` response header, e.g. `Content-Length`.
fn header_u64(headers: &[(String, String)], name: &str) -> Option<u64> {
    header(headers, name).and_then(|s| s.trim().parse::<u64>().ok())
}

/// [`StreamSink`] that hashes and writes each chunk, tracks [`Progress`], and honors
/// cancellation. App-level failures (non-2xx status, write error, cancellation) are recorded in
/// `fail` — the transport error that aborts the stream is a placeholder, and the caller surfaces
/// `fail` instead.
struct HashWriteSink<'a, W: Write> {
    sink: &'a mut W,
    progress: &'a Progress,
    hasher: Sha256,
    fallback_total: u64,
    fail: Option<String>,
}

impl<W: Write> StreamSink for HashWriteSink<'_, W> {
    fn head(&mut self, status: u16, headers: &[(String, String)]) -> bool {
        if !(200..300).contains(&status) {
            self.fail = Some(format!("request failed: HTTP {status}"));
            return false;
        }
        let total = header_u64(headers, "Content-Length").unwrap_or(self.fallback_total);
        self.progress.total.store(total, Ordering::Relaxed);
        self.progress.downloaded.store(0, Ordering::Relaxed);
        true
    }
    fn chunk(&mut self, data: &[u8]) -> Result<(), HttpError> {
        if self.progress.is_cancelled() {
            self.fail = Some("cancelled".to_string());
            return Err(HttpError::Io("cancelled".into()));
        }
        self.hasher.update(data);
        if let Err(e) = self.sink.write_all(data) {
            self.fail = Some(format!("write failed: {e}"));
            return Err(HttpError::Io("write failed".into()));
        }
        self.progress
            .downloaded
            .fetch_add(data.len() as u64, Ordering::Relaxed);
        Ok(())
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
    let mut s = HashWriteSink {
        sink,
        progress,
        hasher: Sha256::new(),
        fallback_total,
        fail: None,
    };
    match day_part_http::fetch_streamed(&Request::get(url), &mut s) {
        Ok(_) => Ok(hex_lower(&s.hasher.finalize())),
        Err(e) => Err(s
            .fail
            .take()
            .unwrap_or_else(|| format!("request failed: {e}"))),
    }
}

/// [`StreamSink`] for [`download_to_file_resumable`]: decides append-vs-restart when the response
/// head arrives (a `206` resumes the partial on disk — seeding the hasher from it — anything else
/// in 2xx truncates and restarts), then hashes/writes/reports like [`HashWriteSink`].
struct ResumeSink<'a> {
    path: &'a std::path::Path,
    progress: &'a Progress,
    fallback_total: u64,
    /// Partial bytes already on disk when a resume is attempted; 0 forces the fresh path.
    existing: u64,
    hasher: Sha256,
    file: Option<std::fs::File>,
    downloaded: u64,
    wrote_any: bool,
    fail: Option<String>,
}

impl<'a> ResumeSink<'a> {
    fn new(
        path: &'a std::path::Path,
        progress: &'a Progress,
        fallback_total: u64,
        existing: u64,
    ) -> Self {
        Self {
            path,
            progress,
            fallback_total,
            existing,
            hasher: Sha256::new(),
            file: None,
            downloaded: 0,
            wrote_any: false,
            fail: None,
        }
    }
}

impl StreamSink for ResumeSink<'_> {
    fn head(&mut self, status: u16, headers: &[(String, String)]) -> bool {
        let resume = self.existing > 0 && status == 206;
        if !resume && !(200..300).contains(&status) {
            self.fail = Some(format!("request failed: HTTP {status}"));
            return false;
        }
        let (downloaded, total) = if resume {
            // The full size is the `/total` in `Content-Range: bytes start-end/total`.
            let total = header(headers, "Content-Range")
                .and_then(|v| v.rsplit('/').next())
                .and_then(|t| t.trim().parse::<u64>().ok())
                .or_else(|| header_u64(headers, "Content-Length").map(|l| self.existing + l))
                .unwrap_or(self.fallback_total);
            // Seed the hasher with the on-disk bytes so the digest covers the whole file.
            if let Err(e) = seed_hasher_from_file(self.path, &mut self.hasher) {
                self.fail = Some(e);
                return false;
            }
            match std::fs::OpenOptions::new().append(true).open(self.path) {
                Ok(f) => self.file = Some(f),
                Err(e) => {
                    self.fail = Some(format!("open failed: {e}"));
                    return false;
                }
            }
            (self.existing, total)
        } else {
            // Fresh download — or the server ignored `Range` and answered `200`, in which case
            // the body is the whole file: truncate and restart cleanly from zero.
            self.hasher = Sha256::new();
            match std::fs::File::create(self.path) {
                Ok(f) => self.file = Some(f),
                Err(e) => {
                    self.fail = Some(format!("create file: {e}"));
                    return false;
                }
            }
            (
                0,
                header_u64(headers, "Content-Length").unwrap_or(self.fallback_total),
            )
        };
        self.downloaded = downloaded;
        self.progress.total.store(total, Ordering::Relaxed);
        self.progress
            .downloaded
            .store(downloaded, Ordering::Relaxed);
        true
    }

    fn chunk(&mut self, data: &[u8]) -> Result<(), HttpError> {
        if self.progress.is_cancelled() {
            self.fail = Some("cancelled".to_string());
            return Err(HttpError::Io("cancelled".into()));
        }
        let Some(file) = self.file.as_mut() else {
            return Err(HttpError::Io("no open file".into()));
        };
        self.hasher.update(data);
        if let Err(e) = file.write_all(data) {
            self.fail = Some(format!("write failed: {e}"));
            return Err(HttpError::Io("write failed".into()));
        }
        self.wrote_any = true;
        self.downloaded += data.len() as u64;
        self.progress
            .downloaded
            .store(self.downloaded, Ordering::Relaxed);
        Ok(())
    }
}

/// Download `url` to `path`, **resuming** an existing partial file with an HTTP `Range` request
/// when one is present (#11). Streams in chunks (a large APK never sits in memory), reports
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

    // Ask to resume when a partial file exists. A failure at or before the response head falls
    // back to a fresh GET; once bytes are moving (or the user cancelled), the error is final.
    if existing > 0 {
        let mut sink = ResumeSink::new(path, progress, fallback_total, existing);
        match day_part_http::fetch_streamed(
            &Request::get(url).header("Range", &format!("bytes={existing}-")),
            &mut sink,
        ) {
            Ok(_) => return Ok(hex_lower(&sink.hasher.finalize())),
            Err(e) => {
                if sink.wrote_any || sink.fail.as_deref() == Some("cancelled") {
                    return Err(sink
                        .fail
                        .take()
                        .unwrap_or_else(|| format!("request failed: {e}")));
                }
            }
        }
    }

    let mut sink = ResumeSink::new(path, progress, fallback_total, 0);
    match day_part_http::fetch_streamed(&Request::get(url), &mut sink) {
        Ok(_) => Ok(hex_lower(&sink.hasher.finalize())),
        Err(e) => Err(sink
            .fail
            .take()
            .unwrap_or_else(|| format!("request failed: {e}"))),
    }
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
