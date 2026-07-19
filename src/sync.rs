//! Catalog sync. Fetches the small `entry.json` first: if the timestamp is unchanged (and so is the
//! locale) the repo is up to date for one tiny request. Otherwise it obtains the new index —
//! preferring an **incremental** JSON Merge Patch diff over a full re-download when `entry.json`
//! offers one for our stored timestamp — caches the raw index, and applies it to SQLite with
//! [`crate::db::upsert_index`]. Any problem with the diff falls back to a full download.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use diesel::sqlite::SqliteConnection;

use crate::cache;
use crate::fdroid::{Entry, FileRef, IndexV2};
use crate::net::{self, Progress};
use crate::state::{self, SyncUi};

/// The locale used to pick localized index fields — the device's, so catalog descriptions and
/// screenshots match the device language. `fdroid::pick` falls back (e.g. `fr-FR` → `fr` → `en-US`
/// → `en`) so partial translations degrade gracefully. F-Droid's primary is `en-US`.
pub fn locale() -> String {
    crate::platform::device_locale()
}

/// Check every enabled repository for updates. Opening the DB ensures the default F-Droid repo
/// exists, so first launch syncs it.
pub fn sync_all_enabled() {
    for repo in crate::state::repos() {
        if repo.enabled {
            start(repo.address.clone(), "index-v2.json".to_string());
        }
    }
}

/// A background update check (#7): sync every enabled repo, then count how many of `installed`
/// (package, on-device versionCode) have a newer version in the catalog. Synchronous — the
/// WorkManager worker calls it on its own thread and posts a notification with the count. Sync
/// errors on any one repo are ignored so a single unreachable mirror doesn't sink the whole check.
// Called only from the Android `UpdateWorker` JNI export; dead on desktop/mock builds.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn check_updates_now(installed: &[(String, i64)]) -> i64 {
    let repos = match crate::db::open(&state::db_path()) {
        Ok(mut conn) => crate::db::repos(&mut conn).unwrap_or_default(),
        Err(_) => return 0,
    };
    for repo in repos.iter().filter(|r| r.enabled) {
        let _ = run(&repo.address, "index-v2.json");
    }
    let mut conn = match crate::db::open(&state::db_path()) {
        Ok(conn) => conn,
        Err(_) => return 0,
    };
    installed
        .iter()
        .filter(|(pkg, code)| {
            crate::db::summary_by_pkg(&mut conn, pkg).is_some_and(|s| s.version_code > *code)
        })
        .count() as i64
}

/// Sync one repository in the background. `address` is the base (no index filename); `index_file`
/// is the index filename (e.g. `index-v2.json`).
pub fn start(address: String, index_file: String) {
    thread::spawn(move || {
        crate::util::lower_priority();
        if let Err(e) = run(&address, &index_file) {
            state::set_sync(SyncUi::Error(e));
        }
    });
}

fn run(address: &str, index_file: &str) -> Result<(), String> {
    state::set_sync(SyncUi::Checking);

    // Our own connection for this thread (WAL lets the main thread keep reading).
    let mut conn = crate::db::open(&state::db_path()).map_err(|e| format!("db: {e}"))?;
    let repo_id = crate::db::ensure_repo(&mut conn, address).map_err(|e| format!("db: {e}"))?;
    let stored = crate::db::repo_timestamp(&mut conn, repo_id);
    let locale = locale();
    let synced_locale = crate::db::get_meta(&mut conn, "catalog_locale").unwrap_or_default();
    let locale_changed = synced_locale != locale;
    // A stored-shape upgrade (e.g. gaining `rank`) forces a re-import so the new field backfills.
    let synced_epoch = crate::db::get_meta(&mut conn, "catalog_epoch").unwrap_or_default();
    let epoch_changed = synced_epoch != crate::db::CATALOG_EPOCH;
    let reimport = locale_changed || epoch_changed;
    let fingerprint = crate::db::repo_fingerprint(&mut conn, repo_id);
    // The primary host followed by any mirrors this repo declared, tried in order (#12).
    let bases = net::bases(address, &crate::db::repo_mirrors(&mut conn, repo_id));

    // entry: the current index pointer + available diffs. Verified via the signed entry.jar when a
    // fingerprint is pinned (#1); a pinned repo whose signature fails is an error, never a silent
    // unsigned fallback. Missing/garbled entry → a full (still hash-checked) download below.
    let entry: Option<Entry> = fetch_entry(&bases, &fingerprint)?;

    let up_to_date = entry
        .as_ref()
        .is_some_and(|e| e.timestamp == stored && stored != 0);

    // Nothing changed at all.
    if up_to_date && !reimport {
        state::set_sync(SyncUi::UpToDate);
        return Ok(());
    }

    // Only the locale or the stored shape changed: re-import the cached index — no download. If
    // there's no usable cache, this falls through to a full download below.
    if up_to_date
        && reimport
        && let Some(cached) = cache::read_index(address)
        && let Ok(index) = serde_json::from_slice::<IndexV2>(&cached)
    {
        return finish(&mut conn, repo_id, &index, &locale, true);
    }

    // Obtain the new index (incremental diff when possible, else full), cache it, and apply.
    state::set_sync(SyncUi::Downloading(None));
    let progress = Progress::new();
    let (bytes, index) = obtain_index(
        address,
        &bases,
        index_file,
        entry.as_ref(),
        stored,
        &progress,
    )?;
    cache::write_index(address, &bytes);

    state::set_sync(SyncUi::Building);
    finish(&mut conn, repo_id, &index, &locale, reimport)
}

/// Fetch the repo's entry: the verified signed `entry.jar` when a fingerprint is pinned (and this
/// platform can verify signatures), else the plain `entry.json`. A pinned repo whose signature
/// fails to verify is an error — we never fall back to an unverified index (#1). A missing/garbled
/// (but authenticated) entry parses to `None`, so the caller does a full download.
fn fetch_entry(bases: &[String], fingerprint: &str) -> Result<Option<Entry>, String> {
    if !fingerprint.is_empty() && crate::platform::verifies_signatures() {
        let jar =
            net::get_bytes_from(bases, "/entry.jar").map_err(|e| format!("entry.jar: {e}"))?;
        match crate::platform::verify_entry_jar(&jar, fingerprint) {
            Some(json) => Ok(serde_json::from_str::<Entry>(&json).ok()),
            None => Err("This repository's signature could not be verified.".to_string()),
        }
    } else {
        Ok(net::get_string_from(bases, "/entry.json")
            .ok()
            .and_then(|body| serde_json::from_str::<Entry>(&body).ok()))
    }
}

/// Store the index and record the sync locale.
fn finish(
    conn: &mut SqliteConnection,
    repo_id: i64,
    index: &IndexV2,
    locale: &str,
    force: bool,
) -> Result<(), String> {
    let changed = crate::db::upsert_index(conn, repo_id, index, locale, force)
        .map_err(|e| format!("store: {e}"))?;
    // Record the locale this catalog data was picked for, so a later locale change is detected, and
    // the stored-shape epoch, so a later App Fair upgrade knows this data predates a new field.
    let _ = crate::db::set_meta(conn, "catalog_locale", locale);
    let _ = crate::db::set_meta(conn, "catalog_epoch", crate::db::CATALOG_EPOCH);
    // Refresh the icon loader's mirror-fallback map from the current repos, so icons/screenshots a
    // metadata-only repo doesn't host fall back to its mirrors (#12). Done before the catalog bump
    // that re-renders the list (and requests icons).
    crate::icons::set_mirror_bases(mirror_map(conn));
    state::bump_catalog();
    state::set_sync(SyncUi::Done(changed));
    Ok(())
}

/// Each repo's address paired with its mirror bases (primary-first), for the icon loader's image
/// fallback.
fn mirror_map(conn: &mut SqliteConnection) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for r in crate::db::repos(conn).unwrap_or_default() {
        let mirrors = crate::db::repo_mirrors(conn, r.id);
        out.push((r.address, mirrors));
    }
    out
}

/// Get the current index bytes + parsed index. Prefers an incremental diff (`entry.diffs[stored]`)
/// applied to the cached index; on any problem — no cache, missing/corrupt diff, unparseable merge
/// — falls back to a full download.
fn obtain_index(
    address: &str,
    bases: &[String],
    index_file: &str,
    entry: Option<&Entry>,
    stored: i64,
    progress: &Progress,
) -> Result<(Vec<u8>, IndexV2), String> {
    // Prefer an incremental diff for our stored timestamp; on any problem, fall through to full.
    if stored != 0
        && let Some(diff_ref) = entry.and_then(|e| e.diffs.get(&stored.to_string()))
        && let Some(result) = try_incremental(address, bases, diff_ref)
    {
        return Ok(result);
    }

    // Full download of the current index.
    let index_name = entry
        .map(|e| e.index.name.clone())
        .unwrap_or_else(|| format!("/{index_file}"));
    let index_size = entry.map(|e| e.index.size.max(0) as u64).unwrap_or(0);
    let bytes = download_with_progress(bases, &index_name, index_size, progress)?;
    // When the entry came from a verified `entry.jar`, its index pointer carries a SHA-256 that
    // authenticates these bytes end to end (#1). Reject a mismatch rather than import tampered data.
    if let Some(expected) = entry.map(|e| e.index.sha256.as_str())
        && !expected.is_empty()
        && sha256_hex(&bytes) != expected
    {
        return Err("The downloaded catalog index failed its integrity check.".to_string());
    }
    let index = serde_json::from_slice(&bytes).map_err(|e| format!("parse: {e}"))?;
    Ok((bytes, index))
}

/// Try an incremental update: download the diff, verify it, and merge it onto the cached index.
/// Returns `None` (so the caller re-downloads) on any failure.
fn try_incremental(
    address: &str,
    bases: &[String],
    diff_ref: &FileRef,
) -> Option<(Vec<u8>, IndexV2)> {
    let cached = cache::read_index(address)?;
    let diff_bytes = net::get_bytes_from(bases, &diff_ref.name).ok()?;
    // Verify the DIFF's integrity. The merged index itself can't be hash-checked — our
    // re-serialization won't byte-match F-Droid's canonical JSON — so trust the RFC 7386 merge and
    // validate it by parsing below.
    if !diff_ref.sha256.is_empty() && sha256_hex(&diff_bytes) != diff_ref.sha256 {
        return None;
    }
    let mut base: serde_json::Value = serde_json::from_slice(&cached).ok()?;
    let patch: serde_json::Value = serde_json::from_slice(&diff_bytes).ok()?;
    cache::merge_patch(&mut base, &patch);
    let bytes = serde_json::to_vec(&base).ok()?;
    let index: IndexV2 = serde_json::from_slice(&bytes).ok()?;
    Some((bytes, index))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let mut s = String::with_capacity(64);
    for b in h.finalize() {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Blocking download that pushes `SyncUi::Downloading(fraction)` to the UI while it runs, trying
/// `{base}{path}` for each base until one succeeds (#12).
fn download_with_progress(
    bases: &[String],
    path: &str,
    fallback_total: u64,
    progress: &Progress,
) -> Result<Vec<u8>, String> {
    let finished = Arc::new(AtomicBool::new(false));
    let reporter = {
        let progress = progress.clone();
        let finished = finished.clone();
        thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                state::set_sync(SyncUi::Downloading(progress.fraction()));
                thread::sleep(Duration::from_millis(120));
            }
        })
    };
    let mut last = "no repository URL".to_string();
    let mut out = Err(String::new());
    for base in bases {
        match net::download(&format!("{base}{path}"), progress, fallback_total) {
            // The index isn't hash-checked here (the caller verifies it); drop the digest.
            Ok((bytes, _digest)) => {
                out = Ok(bytes);
                break;
            }
            Err(e) => last = e,
        }
    }
    finished.store(true, Ordering::Relaxed);
    let _ = reporter.join();
    out.map_err(|_| last)
}
