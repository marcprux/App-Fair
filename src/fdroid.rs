//! F-Droid Index V2 parsing (https://gitlab.com/fdroid/wiki/-/wikis/Index-V2).
//!
//! The index is one large JSON object: a `repo` block (name, categories, anti-feature
//! definitions) and a `packages` map keyed by application id. Localized fields are maps keyed by
//! BCP-47 locale; [`pick`] resolves the best match for the running locale. A small `entry.json`
//! sits beside the index and carries the repo timestamp, so a sync can skip the big download when
//! nothing changed.

// The deserialized index carries more than the MVP reads (added dates, source refs, version
// hashes); keep the full shape so features can grow without re-deriving the parser.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::Deserialize;

/// A locale-keyed map, e.g. `{"en-US": "Hello", "fr": "Bonjour"}`.
pub type Localized<T> = BTreeMap<String, T>;

/// Choose the best value for `locale` from a localized map: exact match, then the language prefix
/// (`fr-CA` → `fr`), then `en-US`, then `en`, then whatever comes first.
pub fn pick<'a, T>(map: &'a Localized<T>, locale: &str) -> Option<&'a T> {
    if map.is_empty() {
        return None;
    }
    let lang = locale.split(['-', '_']).next().unwrap_or(locale);
    map.get(locale)
        .or_else(|| {
            map.keys()
                .find(|k| k.starts_with(lang))
                .and_then(|k| map.get(k))
        })
        .or_else(|| map.get("en-US"))
        .or_else(|| map.get("en"))
        .or_else(|| map.values().next())
}

/// The small `entry.json` beside the index: the current index pointer plus, for incremental
/// updates, a map of `<old-timestamp>` → the JSON Merge Patch that upgrades that older index to the
/// current one (https://f-droid.org/2023/03/01/new-repo-format-faster-smaller-updates.html).
#[derive(Debug, Deserialize)]
pub struct Entry {
    pub timestamp: i64,
    pub index: FileRef,
    #[serde(default)]
    pub diffs: BTreeMap<String, FileRef>,
}

/// A file reference inside the index (icon, screenshot, apk, or the index itself).
#[derive(Debug, Deserialize, Clone, Default)]
pub struct FileRef {
    pub name: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Deserialize)]
pub struct IndexV2 {
    pub repo: Repo,
    #[serde(default)]
    pub packages: BTreeMap<String, Package>,
    /// App Fair Index V2 extension: application ids in editorial order, highest-ranked first. Absent
    /// from stock F-Droid indexes (`serde(default)` → empty), so it's an optional lever a catalog
    /// opts into; it drives the "Default" sort order when present (an app's rank is its position).
    #[serde(default)]
    pub rank: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Repo {
    #[serde(default)]
    pub name: Localized<String>,
    pub timestamp: i64,
    #[serde(default)]
    pub categories: BTreeMap<String, CategoryDef>,
    #[serde(default, rename = "antiFeatures")]
    pub anti_features: BTreeMap<String, AntiFeatureDef>,
    /// Mirror base URLs to fall back to when the primary address fails (#12).
    #[serde(default)]
    pub mirrors: Vec<Mirror>,
}

/// A repo mirror. Index V2 entries carry a `url` (the mirror's base repo URL).
#[derive(Debug, Deserialize, Default, Clone)]
pub struct Mirror {
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CategoryDef {
    #[serde(default)]
    pub name: Localized<String>,
}

#[derive(Debug, Deserialize)]
pub struct AntiFeatureDef {
    #[serde(default)]
    pub name: Localized<String>,
    #[serde(default)]
    pub description: Localized<String>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    #[serde(default)]
    pub metadata: Metadata,
    #[serde(default)]
    pub versions: BTreeMap<String, Version>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Metadata {
    #[serde(default)]
    pub name: Localized<String>,
    #[serde(default)]
    pub summary: Localized<String>,
    #[serde(default)]
    pub description: Localized<String>,
    #[serde(default)]
    pub icon: Localized<FileRef>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub categories: Vec<String>,
    // Author fields are FLAT in Index V2 (`authorName`/`authorWebSite`), not a nested `author`
    // object — reading them as an object is why the author showed as "Unknown".
    #[serde(default, rename = "authorName")]
    pub author_name: String,
    #[serde(default, rename = "authorWebSite")]
    pub author_website: String,
    #[serde(default, rename = "webSite")]
    pub website: String,
    #[serde(default, rename = "sourceCode")]
    pub source_code: String,
    /// `{"phone": {"en-US": [FileRef, …]}}` and other form factors.
    #[serde(default)]
    pub screenshots: BTreeMap<String, Localized<Vec<FileRef>>>,
    #[serde(default)]
    pub added: i64,
    #[serde(default, rename = "lastUpdated")]
    pub last_updated: i64,
}

#[derive(Debug, Deserialize)]
pub struct Version {
    #[serde(default)]
    pub file: FileRef,
    #[serde(default)]
    pub manifest: Manifest,
    #[serde(default, rename = "antiFeatures")]
    pub anti_features: BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "whatsNew")]
    pub whats_new: Localized<String>,
}

/// The signing certificate(s) declared for a version.
#[derive(Debug, Deserialize, Default)]
pub struct Signer {
    #[serde(default)]
    pub sha256: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Manifest {
    #[serde(default, rename = "versionName")]
    pub version_name: String,
    #[serde(default, rename = "versionCode")]
    pub version_code: i64,
    #[serde(default, rename = "usesSdk")]
    pub uses_sdk: UsesSdk,
    #[serde(default, rename = "maxSdkVersion")]
    pub max_sdk_version: i64,
    /// Native-code ABIs this version ships (empty = universal/pure-Java, runs anywhere).
    #[serde(default)]
    pub nativecode: Vec<String>,
    #[serde(default, rename = "usesPermission")]
    pub uses_permission: Vec<Permission>,
    /// The APK signing certificate(s), as SHA-256 hex — pinned so the installed APK's signer must
    /// match what the catalog promised (#3). F-Droid places this inside the manifest.
    #[serde(default)]
    pub signer: Signer,
}

#[derive(Debug, Deserialize, Default)]
pub struct UsesSdk {
    #[serde(default, rename = "minSdkVersion")]
    pub min_sdk_version: i64,
}

/// `usesPermission` entries are `["name", maxSdk]` arrays in the index. Only the name matters here.
#[derive(Debug)]
pub struct Permission {
    pub name: String,
}

impl<'de> Deserialize<'de> for Permission {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Accept both `["android.permission.X", null]` and `{"name": "…"}` shapes.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Tuple((String, serde_json::Value)),
            Named { name: String },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Tuple((name, _)) => Permission { name },
            Raw::Named { name } => Permission { name },
        })
    }
}

impl Version {
    /// Whether this version can run on a device at `device_sdk` with `device_abis` (#5). `0`/empty
    /// device info means "unknown" and matches anything.
    pub fn compatible(&self, device_sdk: i64, device_abis: &[String]) -> bool {
        let m = &self.manifest;
        let min_ok = device_sdk == 0 || m.uses_sdk.min_sdk_version <= device_sdk;
        let max_ok = device_sdk == 0 || m.max_sdk_version == 0 || m.max_sdk_version >= device_sdk;
        // A version with no nativecode is pure-Java and runs on any ABI.
        let abi_ok = m.nativecode.is_empty()
            || device_abis.is_empty()
            || m.nativecode
                .iter()
                .any(|n| device_abis.iter().any(|a| a == n));
        min_ok && max_ok && abi_ok
    }
}

impl Package {
    /// The highest `versionCode` overall — the store's fallback when device info is unknown.
    pub fn best_version(&self) -> Option<&Version> {
        self.versions
            .values()
            .max_by_key(|v| v.manifest.version_code)
    }

    /// The version the store should offer: the highest `versionCode` that is COMPATIBLE with the
    /// device (min/max SDK + ABI, #5). Falls back to the highest overall when nothing is compatible
    /// (so the detail page can still show it and explain the incompatibility).
    pub fn best_version_for(&self, device_sdk: i64, device_abis: &[String]) -> Option<&Version> {
        self.versions
            .values()
            .filter(|v| v.compatible(device_sdk, device_abis))
            .max_by_key(|v| v.manifest.version_code)
            .or_else(|| self.best_version())
    }

    /// Phone screenshots for `locale`, resolved to their repo-relative paths.
    pub fn screenshot_paths(&self, locale: &str) -> Vec<String> {
        // Prefer phone screenshots; fall back to any form factor present.
        let group = self
            .metadata
            .screenshots
            .get("phone")
            .or_else(|| self.metadata.screenshots.values().next());
        group
            .and_then(|by_locale| pick(by_locale, locale))
            .map(|list| list.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entry_with_diffs() {
        // entry.json carries the current index pointer plus a `<old-timestamp>` → diff map.
        let entry: Entry = serde_json::from_str(include_str!("testdata/entry.json")).unwrap();
        assert_eq!(entry.timestamp, 2000);
        assert_eq!(entry.index.name, "/index-v2.json");
        assert_eq!(entry.diffs.get("1000").unwrap().name, "/diff/1000.json");
    }

    #[test]
    fn parses_flat_author_fields() {
        // Index V2 metadata carries the author as flat `authorName`/`authorWebSite`.
        let json = r#"{
            "metadata": {
                "authorName": "Guardian Project",
                "authorWebSite": "https://guardianproject.info",
                "name": {"en-US": "Checkey"}
            },
            "versions": {}
        }"#;
        let pkg: Package = serde_json::from_str(json).unwrap();
        assert_eq!(pkg.metadata.author_name, "Guardian Project");
        assert_eq!(pkg.metadata.author_website, "https://guardianproject.info");
    }
}
