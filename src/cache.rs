//! On-disk cache of each catalog's raw `index-v2.json`, plus the JSON Merge Patch that drives
//! incremental updates. Keeping the raw index means we can re-import a catalog (e.g. after a locale
//! change) with no download, and it is the base a downloaded diff patches into the new index.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde_json::Value;

/// The cache file for a repo's raw index, named by a hash of its address (like the image cache).
fn index_cache_path(repo_address: &str) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    repo_address.hash(&mut h);
    crate::platform::data_dir()
        .join("catalogs")
        .join(format!("{:016x}.json", h.finish()))
}

/// The cached raw index bytes for `repo_address`, or `None` if nothing is cached yet.
pub fn read_index(repo_address: &str) -> Option<Vec<u8>> {
    let bytes = std::fs::read(index_cache_path(repo_address)).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

/// Persist the raw index bytes for `repo_address` (best-effort).
pub fn write_index(repo_address: &str, bytes: &[u8]) {
    let path = index_cache_path(repo_address);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, bytes);
}

/// Drop a repo's cached index (when the repo is removed).
pub fn remove_index(repo_address: &str) {
    let _ = std::fs::remove_file(index_cache_path(repo_address));
}

/// Apply an RFC 7386 JSON Merge Patch: recursively merge `patch` into `target`. A `null` value in
/// the patch deletes that key; a non-object patch replaces the target outright.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(patch_map) => {
            if !target.is_object() {
                *target = Value::Object(serde_json::Map::new());
            }
            let target_map = target.as_object_mut().expect("just set to an object");
            for (k, v) in patch_map {
                if v.is_null() {
                    target_map.remove(k);
                } else {
                    merge_patch(target_map.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        _ => *target = patch.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::merge_patch;
    use serde_json::json;

    #[test]
    fn merges_and_deletes() {
        // RFC 7386 examples: replace a value, add a key, delete a key with null, recurse.
        let mut target = json!({
            "a": "old",
            "b": {"keep": 1, "drop": 2},
            "gone": "x"
        });
        let patch = json!({
            "a": "new",
            "b": {"drop": null, "add": 3},
            "gone": null,
            "c": "added"
        });
        merge_patch(&mut target, &patch);
        assert_eq!(
            target,
            json!({
                "a": "new",
                "b": {"keep": 1, "add": 3},
                "c": "added"
            })
        );
    }

    #[test]
    fn non_object_patch_replaces() {
        let mut target = json!({"was": "object"});
        merge_patch(&mut target, &json!("scalar"));
        assert_eq!(target, json!("scalar"));
    }
}
