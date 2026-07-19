//! Plain data the UI and storage layers share. These are the shapes read out of SQLite and
//! handed to the pages — deliberately flat and `Clone`, never the raw index JSON.

// These structs mirror the full catalog record; a few fields (APK hash, min-SDK, timestamps) are
// stored for features not yet surfaced in the MVP UI — hash verification, compatibility gating,
// and sort orders.
#![allow(dead_code)]

/// One row in the browse/search list: enough to draw a list cell without touching the detail data.
#[derive(Clone, Debug)]
pub struct AppSummary {
    pub repo_id: i64,
    /// Android application id, e.g. `org.fdroid.fdroid`.
    pub pkg: String,
    pub name: String,
    pub summary: String,
    /// Repo-relative icon path (`/org.fdroid.fdroid/en-US/icon.png`), or empty.
    pub icon_path: String,
    /// Repo base address the icon path resolves against.
    pub repo_address: String,
    /// Newest available version name (for the list's right-hand column).
    pub version_name: String,
    pub version_code: i64,
}

impl AppSummary {
    /// Absolute icon URL, or `None` when the app ships no icon.
    pub fn icon_url(&self) -> Option<String> {
        if self.icon_path.is_empty() {
            None
        } else {
            Some(format!("{}{}", self.repo_address, self.icon_path))
        }
    }
}

/// The detail page's data: everything the store shows about one app.
#[derive(Clone, Debug, Default)]
pub struct AppDetail {
    pub repo_id: i64,
    pub pkg: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub license: String,
    pub author: String,
    /// The author's own website (`authorWebSite`), distinct from the app's `website`.
    pub author_website: String,
    pub website: String,
    pub source_code: String,
    pub icon_path: String,
    pub repo_address: String,
    /// Repo-relative screenshot paths.
    pub screenshots: Vec<String>,
    pub version_name: String,
    pub version_code: i64,
    pub apk_path: String,
    pub apk_size: i64,
    pub apk_sha256: String,
    /// SHA-256 of the APK signing certificate the catalog promises (`signer.sha256[0]`), verified
    /// against the downloaded APK before install and shown on the detail page.
    pub signer: String,
    pub min_sdk: i64,
    /// Raw Android permission names from the newest version's manifest.
    pub permissions: Vec<String>,
    /// Anti-feature keys declared by the newest version, resolved to localized name/description.
    pub anti_features: Vec<AntiFeature>,
    pub whats_new: String,
    pub last_updated: i64,
    pub categories: Vec<String>,
}

impl AppDetail {
    pub fn icon_url(&self) -> Option<String> {
        if self.icon_path.is_empty() {
            None
        } else {
            Some(format!("{}{}", self.repo_address, self.icon_path))
        }
    }

    pub fn apk_url(&self) -> String {
        format!("{}{}", self.repo_address, self.apk_path)
    }

    pub fn screenshot_urls(&self) -> Vec<String> {
        self.screenshots
            .iter()
            .map(|p| format!("{}{}", self.repo_address, p))
            .collect()
    }
}

/// A localized anti-feature label, resolved from the repo's `antiFeatures` table.
#[derive(Clone, Debug)]
pub struct AntiFeature {
    pub key: String,
    pub name: String,
    pub description: String,
}

/// A configured repository row.
#[derive(Clone, Debug)]
pub struct RepoRow {
    pub id: i64,
    /// Base address without the index filename, e.g. `https://f-droid.org/repo`.
    pub address: String,
    pub name: String,
    /// Repo timestamp of the last successful sync (0 = never synced).
    pub timestamp: i64,
    pub enabled: bool,
    pub app_count: i64,
    /// SHA-256 fingerprint of the repo's signing certificate (empty = unverified repo).
    pub fingerprint: String,
    /// Precedence when the same package is in several repos (higher wins).
    pub priority: i64,
}

/// A category, resolved to its localized name.
#[derive(Clone, Debug)]
pub struct Category {
    pub key: String,
    pub name: String,
}

/// How the catalog list is ordered, chosen from the sort drop-down.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SortOrder {
    /// The catalog's editorial `rank`, falling back to recency for apps a catalog didn't rank (and
    /// for whole catalogs without the `rank` extension). While a search query is active this keeps
    /// relevance first, with rank/recency as the tie-break.
    #[default]
    Default,
    /// Most recently updated first — ignores rank and relevance.
    LastUpdated,
    /// Alphabetical by name, using the device locale's collation (see `crate::collate`).
    Name,
}

/// Split an index URL (`https://host/repo/index-v2.json`) into (base address, index filename).
/// The base is everything up to the final `/`; the filename defaults to `index-v2.json`.
pub fn split_index_url(url: &str) -> (String, String) {
    let trimmed = url.trim().trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((base, last)) if last.ends_with(".json") => (base.to_string(), last.to_string()),
        _ => (trimmed.to_string(), "index-v2.json".to_string()),
    }
}
