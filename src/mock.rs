//! A bundled F-Droid catalog subset for offline, deterministic runs — local testing and CI
//! screenshot generation without downloading the full ~50 MB index. Gated by the `APP_FAIR_MOCK`
//! environment variable (the CI passes it through `launch-env`, and it reaches the app as a normal
//! env var). When it's set, `root()` calls [`seed`] to load the embedded `mock/catalog.json` into
//! the catalog and pre-warm the icon cache from `mock/icons/`, instead of syncing over the
//! network — so the walkthrough always shows the same tasteful set of apps.

use crate::fdroid::{self, IndexV2};
use crate::{catalog, icons, model, state};

/// The curated catalog (12 well-known, non-controversial F-Droid apps, real metadata), embedded at
/// build time from a verbatim subset of the F-Droid Index V2.
const CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/mock/catalog.json"));

/// Each app's real icon, embedded so the seeded catalog renders fully offline — `(pkg, PNG bytes)`.
const ICONS: &[(&str, &[u8])] = &[
    (
        "app.organicmaps",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/app.organicmaps.png"
        )),
    ),
    (
        "com.ichi2.anki",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/com.ichi2.anki.png"
        )),
    ),
    (
        "com.keylesspalace.tusky",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/com.keylesspalace.tusky.png"
        )),
    ),
    (
        "de.danoeh.antennapod",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/de.danoeh.antennapod.png"
        )),
    ),
    (
        "eu.faircode.email",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/eu.faircode.email.png"
        )),
    ),
    (
        "net.gsantner.markor",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/net.gsantner.markor.png"
        )),
    ),
    (
        "org.breezyweather",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/org.breezyweather.png"
        )),
    ),
    (
        "org.fdroid.fdroid",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/org.fdroid.fdroid.png"
        )),
    ),
    (
        "org.fossify.gallery",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/org.fossify.gallery.png"
        )),
    ),
    (
        "org.mozilla.fennec_fdroid",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/org.mozilla.fennec_fdroid.png"
        )),
    ),
    (
        "org.videolan.vlc",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/org.videolan.vlc.png"
        )),
    ),
    (
        "org.wikipedia",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/mock/icons/org.wikipedia.png"
        )),
    ),
];

/// Whether to run against the bundled catalog instead of the network — set by `APP_FAIR_MOCK`
/// (any non-empty value other than `0`).
pub fn enabled() -> bool {
    std::env::var("APP_FAIR_MOCK").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Seed the catalog and icon cache from the embedded catalog at launch, replacing any prior sync
/// for the default repo. The icons land on disk before the import commits, so the list's first
/// rows find them. Safe to call again.
pub fn seed() {
    let Ok(index) = serde_json::from_str::<IndexV2>(CATALOG) else {
        day::warn!("mock catalog failed to parse");
        return;
    };
    // Follow the UI language (set from `DAY_LOCALE`) so a French run shows French catalog fields
    // where F-Droid has them; `fdroid::pick` falls back to `en-US` otherwise.
    let locale = std::env::var("DAY_LOCALE").unwrap_or_else(|_| "en-US".to_string());
    let repo_addr = model::split_index_url(catalog::DEFAULT_INDEX_URL).0;
    preseed_icons(&index, &repo_addr, &locale);
    state::write(move |db| {
        let repo_id = catalog::ensure_repo(db, &repo_addr)?;
        catalog::upsert_index(db, repo_id, &index, &locale, true).map(|_| ())
    });
}

/// Write each app's embedded icon into the on-disk image cache under the exact URL the list will
/// request (`repo_address + icon_path`), so icons render without any network fetch. Icons are
/// locale-independent, but resolve the path the same way `upsert_index` did so the URLs match.
fn preseed_icons(index: &IndexV2, repo_addr: &str, locale: &str) {
    for (pkg, bytes) in ICONS {
        if let Some(package) = index.packages.get(*pkg)
            && let Some(icon) = fdroid::pick(&package.metadata.icon, locale)
        {
            icons::preseed(&format!("{repo_addr}{}", icon.name), bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_and_icons_stay_consistent() {
        let index: IndexV2 = serde_json::from_str(CATALOG).expect("mock catalog parses");
        assert!(
            index.packages.len() >= 10,
            "expected a dozen curated apps, got {}",
            index.packages.len()
        );
        // Every package ships a non-empty icon so the seeded list renders fully offline...
        let icon_pkgs: HashSet<&str> = ICONS.iter().map(|(p, _)| *p).collect();
        for pkg in index.packages.keys() {
            assert!(
                icon_pkgs.contains(pkg.as_str()),
                "no bundled icon for {pkg}"
            );
        }
        // ...and every bundled icon maps to a package (no orphans).
        for (pkg, bytes) in ICONS {
            assert!(
                index.packages.contains_key(*pkg),
                "icon for missing pkg {pkg}"
            );
            assert!(!bytes.is_empty(), "empty icon for {pkg}");
        }
    }

    #[test]
    fn mock_catalog_seeds_into_the_catalog() {
        let index: IndexV2 = serde_json::from_str(CATALOG).unwrap();
        let db = catalog::open(day_persistence::Sqlite::memory()).unwrap();
        let repo_addr = model::split_index_url(catalog::DEFAULT_INDEX_URL).0;
        let repo_id = catalog::ensure_repo(&db, &repo_addr).unwrap();
        let written = catalog::upsert_index(&db, repo_id, &index, "en-US", true).unwrap();
        assert_eq!(written, index.packages.len());
        // A known app round-trips through the real query path.
        let wiki = catalog::detail(&db, repo_id, "org.wikipedia")
            .unwrap()
            .unwrap();
        assert_eq!(wiki.name, "Wikipedia");
        assert!(!wiki.signer.is_empty(), "signer parsed from the manifest");
        // The walkthrough opens Organic Maps by its row key, and expects it in the default
        // catalog with id 1.
        assert_eq!(repo_id, 1);
        assert!(
            catalog::detail(&db, repo_id, "app.organicmaps")
                .unwrap()
                .is_some()
        );
    }
}
