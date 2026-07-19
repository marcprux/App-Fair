//! App-wide state: the reactive signals the pages read, the SQLite connection the main thread
//! queries, and the `Setter`s background work uses to push results back. Signals live in a
//! detached scope so they outlive the page that first reads them; the setters are stored globally
//! because a download thread or the install callback needs them from off the main thread.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use day::prelude::*;
use diesel::sqlite::SqliteConnection;

use crate::db;
use crate::model::{AppDetail, AppSummary, Category, RepoRow, SortOrder};
use crate::platform;

/// Remaining apps in a "Update all" batch — each installs after the previous reaches a terminal
/// state (#8). Empty for a normal single install, so it never affects that path.
static UPDATE_QUEUE: Mutex<Vec<AppDetail>> = Mutex::new(Vec::new());

/// The default catalog: the App Fair Index V2 repository.
pub const DEFAULT_INDEX_URL: &str = "https://appfair.net/repo/index-v2.json";

/// The App Fair repo's pinned signing-certificate fingerprint (SHA-256 of the signing cert, the
/// value `verifyEntryJar` derives), so the default catalog's signed index is verified (#1).
const DEFAULT_FINGERPRINT: &str =
    "ddbbea5229957f75299c485d3b831ac30598459ad2ac69c293867163b4ed3c71";

/// Progress of a catalog sync, shown in the list header.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncUi {
    Idle,
    Checking,
    /// Download fraction, or `None` when the size is unknown.
    Downloading(Option<f32>),
    Building,
    UpToDate,
    Done(usize),
    Error(String),
}

/// Progress of an app download + install, shown in the install sheet.
#[derive(Clone, Debug)]
pub struct InstallUi {
    /// The package being installed (identifies the in-flight app; not shown directly).
    #[allow(dead_code)]
    pub pkg: String,
    pub name: String,
    pub phase: InstallPhase,
    pub progress: f32,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InstallPhase {
    Downloading,
    Installing,
    Success,
    Failed(String),
    Cancelled,
}

struct State {
    query: Signal<String>,
    category: Signal<Option<String>>,
    /// The catalog to browse (`None` = all enabled catalogs).
    repo_filter: Signal<Option<i64>>,
    /// The catalog list's sort order (the sort drop-down).
    sort: Signal<SortOrder>,
    catalog_version: Signal<u64>,
    sync: Signal<SyncUi>,
    install: Signal<Option<InstallUi>>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// Main-thread SQLite handle, opened once.
    static DB: RefCell<Option<SqliteConnection>> = const { RefCell::new(None) };
}

// Cross-thread write handles, populated at init on the main thread.
static SYNC_SETTER: OnceLock<Setter<SyncUi>> = OnceLock::new();
static INSTALL_SETTER: OnceLock<Setter<Option<InstallUi>>> = OnceLock::new();
static CATALOG_SETTER: OnceLock<Setter<u64>> = OnceLock::new();
/// The app currently downloading/installing, so the install callback can label its result and
/// clean up the staged APK once the installer is done with it.
#[derive(Clone)]
struct InFlight {
    pkg: String,
    name: String,
    /// The catalog this app is being installed from, recorded on success so App Fair knows it owns
    /// the install and which source it came from (#9).
    repo_id: i64,
    /// The downloaded APK on disk, deleted once the install reaches a terminal state.
    apk_path: PathBuf,
}
static IN_FLIGHT: Mutex<Option<InFlight>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|cell| {
        if cell.borrow().is_none() {
            // Detached scope: these signals must outlive the page that first reads them.
            let st = Scope::detached().enter(|| State {
                query: Signal::new(String::new()),
                category: Signal::new(None),
                repo_filter: Signal::new(None),
                sort: Signal::new(SortOrder::default()),
                catalog_version: Signal::new(0),
                sync: Signal::new(SyncUi::Idle),
                install: Signal::new(None),
            });
            let _ = SYNC_SETTER.set(st.sync.setter());
            let _ = INSTALL_SETTER.set(st.install.setter());
            let _ = CATALOG_SETTER.set(st.catalog_version.setter());
            *cell.borrow_mut() = Some(st);
        }
        f(cell.borrow().as_ref().unwrap())
    })
}

pub fn query() -> Signal<String> {
    with_state(|s| s.query)
}
pub fn category() -> Signal<Option<String>> {
    with_state(|s| s.category)
}
pub fn repo_filter() -> Signal<Option<i64>> {
    with_state(|s| s.repo_filter)
}
pub fn sort_order() -> Signal<SortOrder> {
    with_state(|s| s.sort)
}
pub fn catalog_version() -> Signal<u64> {
    with_state(|s| s.catalog_version)
}
pub fn sync_signal() -> Signal<SyncUi> {
    with_state(|s| s.sync)
}
pub fn install_signal() -> Signal<Option<InstallUi>> {
    with_state(|s| s.install)
}

/// Whether a download/install is mid-flight (so an auto-update batch shouldn't barge in).
pub fn install_active() -> bool {
    install_signal().get_untracked().is_some_and(|u| {
        matches!(
            u.phase,
            InstallPhase::Downloading | InstallPhase::Installing
        )
    })
}

// --- database (main thread) --------------------------------------------------

/// The catalog file location: `<app data>/catalog.db`.
pub fn db_path() -> PathBuf {
    platform::data_dir().join("catalog.db")
}

/// Run a closure against the main-thread SQLite connection, opening it on first use. Diesel takes
/// `&mut` for queries, so the closure does too.
pub fn with_db<R>(f: impl FnOnce(&mut SqliteConnection) -> R) -> R {
    DB.with(|cell| {
        if cell.borrow().is_none() {
            let path = db_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match db::open(&path) {
                Ok(mut conn) => {
                    // The default catalog exists from first launch (empty until first sync), with
                    // the App Fair repo's signing fingerprint pinned so its index is verified (#1).
                    let base = crate::model::split_index_url(DEFAULT_INDEX_URL).0;
                    if let Ok(id) = db::ensure_repo(&mut conn, &base)
                        && db::repo_fingerprint(&mut conn, id).is_empty()
                    {
                        let _ = db::set_repo_fingerprint(&mut conn, id, DEFAULT_FINGERPRINT);
                    }
                    *cell.borrow_mut() = Some(conn);
                }
                Err(e) => eprintln!("app-fair: db open failed: {e}"),
            }
        }
        f(cell.borrow_mut().as_mut().expect("db"))
    })
}

pub fn search_apps() -> Vec<AppSummary> {
    let q = query().get();
    let cat = category().get();
    let repo = repo_filter().get();
    let sort = sort_order().get();
    let excluded = crate::settings::excluded_anti_features();
    // The device locale drives the "Name" collation; cached, so this is cheap.
    let locale = platform::device_locale();
    with_db(|conn| {
        db::search(
            conn,
            &q,
            cat.as_deref(),
            repo,
            &excluded,
            sort,
            &locale,
            500,
        )
        .unwrap_or_default()
    })
}

/// The installed-package list tagged with the install generation it was enumerated at.
type InstalledPkgsCache = Option<(u64, Vec<(String, i64)>)>;

thread_local! {
    /// Cached `platform::installed_packages()` result — the slow JNI enumeration.
    static INSTALLED_PKGS: RefCell<InstalledPkgsCache> = const { RefCell::new(None) };
}

/// Bumps only when the set of installed packages can have changed (an install or uninstall). The
/// package enumeration is cached against it so frequent *catalog* bumps (every sync) don't re-run
/// the JNI call.
static INSTALL_GEN: AtomicU64 = AtomicU64::new(0);

/// Invalidate the installed-package cache after an install/uninstall so the next read re-enumerates.
pub fn bump_installed() {
    INSTALL_GEN.fetch_add(1, Ordering::Relaxed);
}

/// The on-device installed packages (pkg, versionCode), enumerated over JNI on the main thread.
/// Cached against [`INSTALL_GEN`]: this JNI call is slow, and the Updates tab reads it from several
/// reactive bindings that re-run on every catalog sync — re-enumerating each time stalls the UI
/// thread. Only an actual install/uninstall invalidates it.
fn cached_installed_packages() -> Vec<(String, i64)> {
    let generation = INSTALL_GEN.load(Ordering::Relaxed);
    if let Some(cached) = INSTALLED_PKGS.with(|cell| {
        cell.borrow()
            .as_ref()
            .filter(|(g, _)| *g == generation)
            .map(|(_, v)| v.clone())
    }) {
        return cached;
    }
    let installed = platform::installed_packages();
    INSTALLED_PKGS.with(|cell| *cell.borrow_mut() = Some((generation, installed.clone())));
    installed
}

/// Installed user apps that are also in the catalog, paired with the on-device versionCode. The
/// slow package enumeration is cached (see [`cached_installed_packages`]); the per-package catalog
/// lookups are cheap (indexed by pkg) and run each call so a sync's newer versions show up.
pub fn installed_updates() -> Vec<(AppSummary, i64)> {
    let installed = cached_installed_packages();
    with_db(|conn| {
        installed
            .into_iter()
            .filter_map(|(pkg, code)| db::summary_by_pkg(conn, &pkg).map(|s| (s, code)))
            .collect()
    })
}

pub fn all_categories() -> Vec<Category> {
    with_db(|conn| db::categories(conn).unwrap_or_default())
}

pub fn repos() -> Vec<RepoRow> {
    with_db(|conn| db::repos(conn).unwrap_or_default())
}

// --- cross-thread pushes -----------------------------------------------------

pub fn set_sync(ui: SyncUi) {
    if let Some(s) = SYNC_SETTER.get() {
        s.set(ui);
    }
}

pub fn set_install(ui: Option<InstallUi>) {
    if let Some(s) = INSTALL_SETTER.get() {
        s.set(ui);
    }
}

/// Bump the catalog version so every list/category binding re-queries the DB.
pub fn bump_catalog() {
    if let Some(s) = CATALOG_SETTER.get() {
        // Read-modify-write off the main thread is unsafe on a Signal; instead push a fresh,
        // monotonically increasing token. Any change is enough to invalidate the bindings.
        s.set(next_token());
    }
}

fn next_token() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Install a batch of updates one at a time (#8): the first starts now, and each subsequent one
/// starts when the previous reaches a terminal state. A no-op if `apps` is empty.
pub fn start_updates(mut apps: Vec<AppDetail>) {
    if apps.is_empty() {
        return;
    }
    let first = apps.remove(0);
    *UPDATE_QUEUE.lock().unwrap() = apps;
    crate::install::start(&first);
}

/// Start the next queued update, if any. Returns whether one was started.
fn start_next_update() -> bool {
    let next = {
        let mut q = UPDATE_QUEUE.lock().unwrap();
        if q.is_empty() {
            None
        } else {
            Some(q.remove(0))
        }
    };
    match next {
        Some(app) => {
            crate::install::start(&app);
            true
        }
        None => false,
    }
}

/// Record the app being installed and the APK staged for it (for result labelling + cleanup).
pub fn set_in_flight(pkg: &str, name: &str, repo_id: i64, apk_path: &Path) {
    *IN_FLIGHT.lock().unwrap() = Some(InFlight {
        pkg: pkg.to_string(),
        name: name.to_string(),
        repo_id,
        apk_path: apk_path.to_path_buf(),
    });
}

fn in_flight() -> Option<InFlight> {
    IN_FLIGHT.lock().unwrap().clone()
}

/// Called from the Android install-result JNI callback (`platform`): maps a PackageInstaller
/// status into the install sheet's final state, and deletes the staged APK once the installer is
/// done with it.
pub fn on_native_install_status(status: i32, message: Option<String>) {
    let record = in_flight();
    let pkg = record.as_ref().map(|r| r.pkg.clone()).unwrap_or_default();
    let name = record.as_ref().map(|r| r.name.clone()).unwrap_or_default();
    let repo_id = record.as_ref().map(|r| r.repo_id).unwrap_or_default();
    // PackageInstaller status codes.
    const STATUS_SUCCESS: i32 = 0;
    const STATUS_PENDING_USER_ACTION: i32 = -1;
    const STATUS_FAILURE_ABORTED: i32 = 3;

    let phase = match status {
        // The system confirm screen is up; keep the sheet in its Installing state and hold the
        // in-flight record for the eventual terminal status.
        STATUS_PENDING_USER_ACTION => InstallPhase::Installing,
        STATUS_SUCCESS => InstallPhase::Success,
        STATUS_FAILURE_ABORTED => InstallPhase::Cancelled,
        _ => InstallPhase::Failed(message.unwrap_or_else(|| "Install failed".into())),
    };

    // On any terminal status the installer has already copied the APK into its own session, so the
    // staged file on disk is no longer needed — delete it (a large APK must not linger) and clear
    // the record. `Installing` (PENDING) keeps both for the result that follows.
    if !matches!(phase, InstallPhase::Installing) {
        if let Some(r) = &record {
            let _ = std::fs::remove_file(&r.apk_path);
        }
        *IN_FLIGHT.lock().unwrap() = None;
    }

    if phase == InstallPhase::Success {
        // Remember App Fair installed this app and from which catalog, so the detail page can offer
        // to open or uninstall it and name its source (#9).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if !pkg.is_empty() {
            with_db(|conn| {
                let _ = db::record_install(conn, &pkg, repo_id, now);
            });
        }
        // The installed set changed — invalidate the package cache so Updates/detail re-read it.
        bump_installed();
        bump_catalog();
    }
    set_install(Some(InstallUi {
        pkg,
        name,
        phase: phase.clone(),
        progress: 1.0,
        downloaded: 0,
        total: 0,
    }));

    // Drive a "Update all" batch (#8): advance to the next update after a finished one; a user
    // cancel abandons the rest of the batch. `Installing` (system confirm still up) is not terminal.
    match phase {
        InstallPhase::Success | InstallPhase::Failed(_) => {
            start_next_update();
        }
        InstallPhase::Cancelled => {
            UPDATE_QUEUE.lock().unwrap().clear();
        }
        InstallPhase::Downloading | InstallPhase::Installing => {}
    }
}
