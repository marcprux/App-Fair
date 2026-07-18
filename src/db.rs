//! The on-device catalog: a SQLite database (bundled via `libsqlite3-sys`, one code path on every
//! target) holding the parsed F-Droid index, accessed through Diesel. The schema lives in
//! `migrations/` and is applied by [`open`]; queries use the Diesel DSL (see `crate::schema`).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use crate::fdroid::{self, IndexV2, Package};
use crate::model::{AntiFeature, AppDetail, AppSummary, Category, RepoRow};
use crate::schema;

/// The embedded schema migrations (from `migrations/`), applied on every [`open`].
const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

/// A queried/insertable `apps` row — the flat storage shape (JSON columns are `String`s here,
/// parsed into `Vec`s only when building [`AppDetail`]).
#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = schema::apps)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct AppRow {
    repo_id: i64,
    pkg: String,
    name: String,
    summary: String,
    description: String,
    license: String,
    author: String,
    author_website: String,
    website: String,
    source_code: String,
    icon_path: String,
    categories: String,
    screenshots: String,
    last_updated: i64,
    version_name: String,
    version_code: i64,
    apk_path: String,
    apk_size: i64,
    apk_sha256: String,
    min_sdk: i64,
    permissions: String,
    anti_features: String,
    whats_new: String,
    signer: String,
}

/// Open (creating if needed) the catalog database, run pending migrations, and set WAL so the main
/// thread keeps reading while a sync thread writes.
pub fn open(path: &Path) -> Result<SqliteConnection, String> {
    let mut conn =
        SqliteConnection::establish(&path.to_string_lossy()).map_err(|e| format!("open: {e}"))?;
    conn.batch_execute("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .map_err(|e| format!("pragma: {e}"))?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| format!("migrate: {e}"))?;
    Ok(conn)
}

/// An in-memory database with the schema applied — for tests.
#[cfg(test)]
pub fn open_in_memory() -> SqliteConnection {
    let mut conn = SqliteConnection::establish(":memory:").expect("in-memory db");
    conn.batch_execute("PRAGMA foreign_keys = ON;")
        .expect("pragma");
    conn.run_pending_migrations(MIGRATIONS).expect("migrate");
    conn
}

// --- key/value metadata ------------------------------------------------------

/// Read a `meta` value (e.g. the locale the catalog was last synced in). `None` if unset.
pub fn get_meta(conn: &mut SqliteConnection, key: &str) -> Option<String> {
    schema::meta::table
        .find(key)
        .select(schema::meta::value)
        .first::<String>(conn)
        .optional()
        .ok()
        .flatten()
}

/// Write a `meta` value.
pub fn set_meta(conn: &mut SqliteConnection, key: &str, value: &str) -> QueryResult<()> {
    use schema::meta;
    diesel::replace_into(meta::table)
        .values((meta::key.eq(key), meta::value.eq(value)))
        .execute(conn)?;
    Ok(())
}

// --- repositories ------------------------------------------------------------

/// Insert a repository (or return the existing one's id) for `address`. A newly-created repo gets
/// the highest priority so a later-added catalog wins dedup over earlier ones (#6).
pub fn ensure_repo(conn: &mut SqliteConnection, address: &str) -> QueryResult<i64> {
    use schema::repos;
    let existed: i64 = repos::table
        .filter(repos::address.eq(address))
        .count()
        .get_result(conn)?;
    diesel::insert_or_ignore_into(repos::table)
        .values(repos::address.eq(address))
        .execute(conn)?;
    let id: i64 = repos::table
        .filter(repos::address.eq(address))
        .select(repos::id)
        .first(conn)?;
    if existed == 0 {
        let next = repos::table
            .select(diesel::dsl::max(repos::priority))
            .first::<Option<i64>>(conn)?
            .unwrap_or(0)
            + 1;
        diesel::update(repos::table.find(id))
            .set(repos::priority.eq(next))
            .execute(conn)?;
    }
    Ok(id)
}

/// The pinned signing-cert fingerprint for a repo (empty = unverified).
pub fn repo_fingerprint(conn: &mut SqliteConnection, id: i64) -> String {
    use schema::repos;
    repos::table
        .find(id)
        .select(repos::fingerprint)
        .first(conn)
        .unwrap_or_default()
}

/// A repo's failover mirror base URLs (empty when it declared none). Tried after the primary host
/// on the index and APK downloads (#12).
pub fn repo_mirrors(conn: &mut SqliteConnection, id: i64) -> Vec<String> {
    use schema::repos;
    repos::table
        .find(id)
        .select(repos::mirrors)
        .first::<String>(conn)
        .map(|s| {
            s.split('\t')
                .filter(|m| !m.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Set a repo's pinned signing-cert fingerprint.
pub fn set_repo_fingerprint(
    conn: &mut SqliteConnection,
    id: i64,
    fingerprint: &str,
) -> QueryResult<()> {
    use schema::repos;
    diesel::update(repos::table.find(id))
        .set(repos::fingerprint.eq(fingerprint))
        .execute(conn)?;
    Ok(())
}

// --- install provenance (#9) -------------------------------------------------

/// Record that App Fair installed `pkg` from `repo_id` at `installed_at` (unix seconds).
pub fn record_install(
    conn: &mut SqliteConnection,
    pkg: &str,
    repo_id: i64,
    installed_at: i64,
) -> QueryResult<()> {
    use schema::installs;
    diesel::replace_into(installs::table)
        .values((
            installs::pkg.eq(pkg),
            installs::repo_id.eq(repo_id),
            installs::installed_at.eq(installed_at),
        ))
        .execute(conn)?;
    Ok(())
}

/// The catalog `pkg` was installed from, if App Fair installed it (`None` otherwise). Used to name
/// the source and offer an uninstall on the detail page (#9).
pub fn install_source(conn: &mut SqliteConnection, pkg: &str) -> Option<i64> {
    use schema::installs;
    installs::table
        .find(pkg.to_string())
        .select(installs::repo_id)
        .first::<i64>(conn)
        .ok()
}

/// A repo's display name (its `name`, falling back to its address). Empty if the repo is gone.
pub fn repo_name(conn: &mut SqliteConnection, id: i64) -> String {
    use schema::repos;
    repos::table
        .find(id)
        .select((repos::name, repos::address))
        .first::<(String, String)>(conn)
        .map(|(name, address)| if name.is_empty() { address } else { name })
        .unwrap_or_default()
}

/// Forget an install record (after an uninstall).
pub fn remove_install(conn: &mut SqliteConnection, pkg: &str) -> QueryResult<()> {
    use schema::installs;
    diesel::delete(installs::table.find(pkg.to_string())).execute(conn)?;
    Ok(())
}

pub fn set_repo_enabled(conn: &mut SqliteConnection, id: i64, enabled: bool) -> QueryResult<()> {
    use schema::repos;
    diesel::update(repos::table.find(id))
        .set(repos::enabled.eq(enabled))
        .execute(conn)?;
    Ok(())
}

pub fn delete_repo(conn: &mut SqliteConnection, id: i64) -> QueryResult<()> {
    use schema::repos;
    diesel::delete(repos::table.find(id)).execute(conn)?;
    Ok(())
}

pub fn repo_timestamp(conn: &mut SqliteConnection, id: i64) -> i64 {
    use schema::repos;
    repos::table
        .find(id)
        .select(repos::timestamp)
        .first(conn)
        .unwrap_or(0)
}

pub fn repos(conn: &mut SqliteConnection) -> QueryResult<Vec<RepoRow>> {
    use schema::{apps, repos};
    let base: Vec<(i64, String, String, i64, bool, String, i64)> = repos::table
        .select((
            repos::id,
            repos::address,
            repos::name,
            repos::timestamp,
            repos::enabled,
            repos::fingerprint,
            repos::priority,
        ))
        .order((repos::priority.desc(), repos::id))
        .load(conn)?;
    let mut out = Vec::with_capacity(base.len());
    for (id, address, name, timestamp, enabled, fingerprint, priority) in base {
        // Few repos, so a per-repo count is simpler than a grouped aggregate.
        let app_count: i64 = apps::table
            .filter(apps::repo_id.eq(id))
            .count()
            .get_result(conn)?;
        out.push(RepoRow {
            id,
            address,
            name,
            timestamp,
            enabled,
            app_count,
            fingerprint,
            priority,
        });
    }
    Ok(out)
}

// --- sync (minimal writes) ---------------------------------------------------

/// Build the flat `apps` row for one package in `locale`.
fn build_app_row(repo_id: i64, pkg: &str, package: &Package, locale: &str) -> AppRow {
    let meta = &package.metadata;
    let name = fdroid::pick(&meta.name, locale)
        .cloned()
        .unwrap_or_else(|| pkg.to_string());
    let summary = fdroid::pick(&meta.summary, locale)
        .cloned()
        .unwrap_or_default();
    let description = fdroid::pick(&meta.description, locale)
        .cloned()
        .unwrap_or_default();
    let icon_path = fdroid::pick(&meta.icon, locale)
        .map(|f| f.name.clone())
        .unwrap_or_default();
    let categories = serde_json::to_string(&meta.categories).unwrap_or_else(|_| "[]".into());
    let screenshots =
        serde_json::to_string(&package.screenshot_paths(locale)).unwrap_or_else(|_| "[]".into());

    // Offer the highest version the device can actually run (#5), falling back to the highest
    // overall when nothing is compatible (the detail page then explains the incompatibility).
    let best = package.best_version_for(
        crate::platform::device_sdk(),
        &crate::platform::device_abis(),
    );
    let (
        version_name,
        version_code,
        apk_path,
        apk_size,
        apk_sha256,
        min_sdk,
        permissions,
        anti_features,
        whats_new,
        signer,
    ) = match best {
        Some(v) => {
            let perms: Vec<String> = v
                .manifest
                .uses_permission
                .iter()
                .map(|p| p.name.clone())
                .collect();
            let anti_keys: Vec<String> = v.anti_features.keys().cloned().collect();
            let whats_new = fdroid::pick(&v.whats_new, locale)
                .cloned()
                .unwrap_or_default();
            (
                v.manifest.version_name.clone(),
                v.manifest.version_code,
                v.file.name.clone(),
                v.file.size,
                v.file.sha256.clone(),
                v.manifest.uses_sdk.min_sdk_version,
                serde_json::to_string(&perms).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&anti_keys).unwrap_or_else(|_| "[]".into()),
                whats_new,
                v.manifest
                    .signer
                    .sha256
                    .first()
                    .cloned()
                    .unwrap_or_default(),
            )
        }
        None => (
            String::new(),
            0,
            String::new(),
            0,
            String::new(),
            0,
            "[]".into(),
            "[]".into(),
            String::new(),
            String::new(),
        ),
    };

    AppRow {
        repo_id,
        pkg: pkg.to_string(),
        name,
        summary,
        description,
        license: meta.license.clone(),
        author: meta.author_name.clone(),
        author_website: meta.author_website.clone(),
        website: meta.website.clone(),
        source_code: meta.source_code.clone(),
        icon_path,
        categories,
        screenshots,
        last_updated: meta.last_updated,
        version_name,
        version_code,
        apk_path,
        apk_size,
        apk_sha256,
        min_sdk,
        permissions,
        anti_features,
        whats_new,
        signer,
    }
}

/// Apply a freshly parsed index to `repo_id`, writing only rows that changed.
///
/// Each app's `last_updated` acts as a change stamp: an app whose stamp already matches the stored
/// value is left untouched — unless `force` (used when the locale changed, so every localized field
/// must be re-picked). Apps missing from the new index are deleted. Categories and anti-feature
/// definitions are replaced wholesale. The whole update runs in one transaction.
pub fn upsert_index(
    conn: &mut SqliteConnection,
    repo_id: i64,
    index: &IndexV2,
    locale: &str,
    force: bool,
) -> QueryResult<usize> {
    use schema::{anti_features, apps, categories, repos};

    conn.transaction(|conn| {
        // Existing change stamps, to skip unchanged apps.
        let existing: HashMap<String, i64> = apps::table
            .filter(apps::repo_id.eq(repo_id))
            .select((apps::pkg, apps::last_updated))
            .load::<(String, i64)>(conn)?
            .into_iter()
            .collect();

        let mut seen = HashSet::new();
        let mut written = 0usize;

        for (pkg, package) in &index.packages {
            seen.insert(pkg.clone());
            let last_updated = package.metadata.last_updated;
            if !force && existing.get(pkg) == Some(&last_updated) {
                continue; // unchanged — skip the write
            }
            let row = build_app_row(repo_id, pkg, package, locale);
            diesel::replace_into(apps::table)
                .values(&row)
                .execute(conn)?;
            written += 1;
        }

        // Delete apps no longer in the index.
        let stale: Vec<String> = apps::table
            .filter(apps::repo_id.eq(repo_id))
            .select(apps::pkg)
            .load::<String>(conn)?
            .into_iter()
            .filter(|p| !seen.contains(p))
            .collect();
        if !stale.is_empty() {
            diesel::delete(
                apps::table
                    .filter(apps::repo_id.eq(repo_id))
                    .filter(apps::pkg.eq_any(&stale)),
            )
            .execute(conn)?;
        }

        // Categories and anti-feature definitions (small; replaced wholesale).
        diesel::delete(categories::table.filter(categories::repo_id.eq(repo_id))).execute(conn)?;
        for (key, def) in &index.repo.categories {
            let name = fdroid::pick(&def.name, locale)
                .cloned()
                .unwrap_or_else(|| key.clone());
            diesel::replace_into(categories::table)
                .values((
                    categories::repo_id.eq(repo_id),
                    categories::key.eq(key),
                    categories::name.eq(name),
                ))
                .execute(conn)?;
        }
        diesel::delete(anti_features::table.filter(anti_features::repo_id.eq(repo_id)))
            .execute(conn)?;
        for (key, def) in &index.repo.anti_features {
            let name = fdroid::pick(&def.name, locale)
                .cloned()
                .unwrap_or_else(|| key.clone());
            let desc = fdroid::pick(&def.description, locale)
                .cloned()
                .unwrap_or_default();
            diesel::replace_into(anti_features::table)
                .values((
                    anti_features::repo_id.eq(repo_id),
                    anti_features::key.eq(key),
                    anti_features::name.eq(name),
                    anti_features::description.eq(desc),
                ))
                .execute(conn)?;
        }

        let repo_name = fdroid::pick(&index.repo.name, locale)
            .cloned()
            .unwrap_or_default();
        // Mirror base URLs (tab-joined) for failover on the next sync/download (#12).
        let mirrors = index
            .repo
            .mirrors
            .iter()
            .map(|m| m.url.trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty())
            .collect::<Vec<_>>()
            .join("\t");
        diesel::update(repos::table.find(repo_id))
            .set((
                repos::timestamp.eq(index.repo.timestamp),
                repos::name.eq(repo_name),
                repos::mirrors.eq(mirrors),
            ))
            .execute(conn)?;

        Ok(written)
    })
}

// --- queries -----------------------------------------------------------------

/// The `(apps subset, repo address)` tuple every browse/search row selects.
type SummaryTuple = (i64, String, String, String, String, String, String, i64);

fn to_summary(t: SummaryTuple) -> AppSummary {
    AppSummary {
        repo_id: t.0,
        pkg: t.1,
        name: t.2,
        summary: t.3,
        icon_path: t.4,
        repo_address: t.5,
        version_name: t.6,
        version_code: t.7,
    }
}

/// Search enabled repos. Empty `query` and `category` returns the most-recently-updated apps.
/// `repo_id` restricts to one catalog (`None` = all enabled catalogs, deduplicated by repo
/// priority, #6). `exclude_anti` hides apps carrying any of those anti-feature keys (#13). Results
/// are relevance-ranked (#15).
pub fn search(
    conn: &mut SqliteConnection,
    query: &str,
    category: Option<&str>,
    repo_id: Option<i64>,
    exclude_anti: &[String],
    limit: i64,
) -> QueryResult<Vec<AppSummary>> {
    use diesel::sql_types::Bool;
    use schema::{apps, repos};

    let q = query.trim();
    let like = format!("%{q}%");
    let mut sel = apps::table
        .inner_join(repos::table)
        .filter(repos::enabled.eq(true))
        .select((
            apps::repo_id,
            apps::pkg,
            apps::name,
            apps::summary,
            apps::icon_path,
            repos::address,
            apps::version_name,
            apps::version_code,
        ))
        .into_boxed();

    if !q.is_empty() {
        sel = sel.filter(
            apps::name
                .like(like.clone())
                .or(apps::summary.like(like.clone()))
                .or(apps::pkg.like(like.clone())),
        );
    }
    if let Some(cat) = category {
        sel = sel.filter(apps::categories.like(format!("%\"{cat}\"%")));
    }
    if let Some(rid) = repo_id {
        sel = sel.filter(apps::repo_id.eq(rid));
    } else {
        // Browsing all catalogs: keep only the winning row per package (highest repo priority, then
        // highest versionCode, then lowest repo id) so a package in several repos shows once.
        sel = sel.filter(diesel::dsl::sql::<Bool>(
            "NOT EXISTS (SELECT 1 FROM apps a2 JOIN repos r2 ON r2.id = a2.repo_id \
             WHERE r2.enabled = 1 AND a2.pkg = apps.pkg AND \
             (r2.priority > repos.priority \
              OR (r2.priority = repos.priority AND a2.version_code > apps.version_code) \
              OR (r2.priority = repos.priority AND a2.version_code = apps.version_code \
                  AND a2.repo_id < apps.repo_id)))",
        ));
    }
    for af in exclude_anti {
        sel = sel.filter(apps::anti_features.not_like(format!("%\"{af}\"%")));
    }

    // Relevance ranking: exact name, then name-prefix, name-contains, summary-contains, recency.
    let sel = if q.is_empty() {
        sel.order(apps::last_updated.desc())
    } else {
        sel.order((
            apps::name.eq(q.to_string()).desc(),
            apps::name.like(format!("{q}%")).desc(),
            apps::name.like(like.clone()).desc(),
            apps::summary.like(like).desc(),
            apps::last_updated.desc(),
        ))
    };

    let rows: Vec<SummaryTuple> = sel.limit(limit).load(conn)?;
    Ok(rows.into_iter().map(to_summary).collect())
}

/// The catalog summary for one package (the winning enabled repo by priority, #6), for the
/// Updates tab.
pub fn summary_by_pkg(conn: &mut SqliteConnection, pkg: &str) -> Option<AppSummary> {
    use schema::{apps, repos};
    apps::table
        .inner_join(repos::table)
        .filter(repos::enabled.eq(true))
        .filter(apps::pkg.eq(pkg))
        .order((repos::priority.desc(), apps::version_code.desc()))
        .select((
            apps::repo_id,
            apps::pkg,
            apps::name,
            apps::summary,
            apps::icon_path,
            repos::address,
            apps::version_name,
            apps::version_code,
        ))
        .first::<SummaryTuple>(conn)
        .optional()
        .ok()
        .flatten()
        .map(to_summary)
}

/// The display name for one app (for the nav-bar title), or `None` if it isn't in the catalog.
pub fn app_name(conn: &mut SqliteConnection, repo_id: i64, pkg: &str) -> Option<String> {
    use schema::apps;
    apps::table
        .find((repo_id, pkg.to_string()))
        .select(apps::name)
        .first::<String>(conn)
        .optional()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
}

/// Distinct categories across enabled repos, with localized names.
pub fn categories(conn: &mut SqliteConnection) -> QueryResult<Vec<Category>> {
    use schema::{categories, repos};
    let rows: Vec<(String, String)> = categories::table
        .inner_join(repos::table)
        .filter(repos::enabled.eq(true))
        .select((categories::key, categories::name))
        .distinct()
        .order(categories::name)
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(key, name)| Category { key, name })
        .collect())
}

/// Load the detail record for one app.
pub fn detail(
    conn: &mut SqliteConnection,
    repo_id: i64,
    pkg: &str,
) -> QueryResult<Option<AppDetail>> {
    use schema::{anti_features, apps, repos};

    let row: Option<(AppRow, String)> = apps::table
        .inner_join(repos::table)
        .filter(apps::repo_id.eq(repo_id))
        .filter(apps::pkg.eq(pkg))
        .select((AppRow::as_select(), repos::address))
        .first(conn)
        .optional()?;

    let Some((r, repo_address)) = row else {
        return Ok(None);
    };

    let mut detail = AppDetail {
        repo_id: r.repo_id,
        pkg: r.pkg,
        name: r.name,
        summary: r.summary,
        description: r.description,
        license: r.license,
        author: r.author,
        author_website: r.author_website,
        website: r.website,
        source_code: r.source_code,
        icon_path: r.icon_path,
        repo_address,
        screenshots: serde_json::from_str(&r.screenshots).unwrap_or_default(),
        version_name: r.version_name,
        version_code: r.version_code,
        apk_path: r.apk_path,
        apk_size: r.apk_size,
        apk_sha256: r.apk_sha256,
        signer: r.signer,
        min_sdk: r.min_sdk,
        permissions: serde_json::from_str(&r.permissions).unwrap_or_default(),
        anti_features: Vec::new(),
        whats_new: r.whats_new,
        last_updated: r.last_updated,
        categories: serde_json::from_str(&r.categories).unwrap_or_default(),
    };

    // Resolve anti-feature keys to localized labels.
    let keys: Vec<String> = serde_json::from_str(&r.anti_features).unwrap_or_default();
    for key in keys {
        let nd: Option<(String, String)> = anti_features::table
            .find((repo_id, key.clone()))
            .select((anti_features::name, anti_features::description))
            .first(conn)
            .optional()?;
        let (name, description) = nd.unwrap_or_else(|| (key.clone(), String::new()));
        detail.anti_features.push(AntiFeature {
            key,
            name,
            description,
        });
    }
    Ok(Some(detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A small F-Droid-compatible catalog subset + a diff, to exercise the ORM and incremental
    // updates without any network.
    const BASE: &str = include_str!("testdata/index-v2.json");
    const DIFF: &str = include_str!("testdata/diff-1000.json");

    const ADDR: &str = "https://test.example/repo";

    fn setup(locale: &str) -> (SqliteConnection, i64) {
        let mut conn = open_in_memory();
        let repo_id = ensure_repo(&mut conn, ADDR).unwrap();
        let index: IndexV2 = serde_json::from_str(BASE).unwrap();
        upsert_index(&mut conn, repo_id, &index, locale, false).unwrap();
        (conn, repo_id)
    }

    #[test]
    fn upsert_and_query() {
        let (mut conn, repo_id) = setup("en-US");

        let r = repos(&mut conn).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].app_count, 2);
        assert_eq!(r[0].timestamp, 1000);
        assert_eq!(r[0].name, "Test Repo");

        // Browse + search + ordering (exact-name matches first).
        assert_eq!(search(&mut conn, "", None, None, &[], 10).unwrap().len(), 2);
        let hits = search(&mut conn, "Alpha", None, None, &[], 10).unwrap();
        assert_eq!(hits[0].pkg, "com.example.alpha");

        // Category filter (matches the JSON array text).
        let sys = search(&mut conn, "", Some("System"), None, &[], 10).unwrap();
        assert_eq!(sys.len(), 1);
        assert_eq!(sys[0].pkg, "com.example.beta");

        // Anti-feature exclusion (#13): hiding "Tracking" drops alpha.
        let filtered = search(&mut conn, "", None, None, &["Tracking".to_string()], 10).unwrap();
        assert!(filtered.iter().all(|a| a.pkg != "com.example.alpha"));

        let cat_names: Vec<String> = categories(&mut conn)
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert!(cat_names.contains(&"Internet".to_string()));
        assert!(cat_names.contains(&"System".to_string()));

        // Detail: authors, JSON-coded fields, and localized anti-features all round-trip.
        let d = detail(&mut conn, repo_id, "com.example.alpha")
            .unwrap()
            .unwrap();
        assert_eq!(d.author, "Alice");
        assert_eq!(d.author_website, "https://alice.example");
        assert_eq!(d.description, "Alpha description");
        assert_eq!(d.apk_size, 111);
        assert_eq!(d.signer, "deadbeefcafe0001"); // #3 signer pinning
        assert_eq!(d.min_sdk, 21);
        assert_eq!(
            d.permissions,
            vec!["android.permission.INTERNET".to_string()]
        );
        assert_eq!(d.categories, vec!["Internet".to_string()]);
        assert_eq!(d.anti_features.len(), 1);
        assert_eq!(d.anti_features[0].name, "Tracking");
        assert_eq!(d.anti_features[0].description, "Tracks you.");

        assert!(summary_by_pkg(&mut conn, "com.example.beta").is_some());
        assert_eq!(
            app_name(&mut conn, repo_id, "com.example.alpha").as_deref(),
            Some("Alpha")
        );
    }

    #[test]
    fn stores_mirrors_from_index() {
        // upsert_index records the repo's mirror base URLs (trailing slash trimmed) for failover.
        let (mut conn, repo_id) = setup("en-US");
        assert_eq!(
            repo_mirrors(&mut conn, repo_id),
            vec![
                "https://mirror1.example/repo".to_string(),
                "https://mirror2.example/repo".to_string(),
            ]
        );
    }

    #[test]
    fn install_provenance_round_trips() {
        // record → source lookup → name → forget (#9).
        let (mut conn, repo_id) = setup("en-US");
        assert!(install_source(&mut conn, "com.example.alpha").is_none());

        record_install(&mut conn, "com.example.alpha", repo_id, 1_700_000_000).unwrap();
        assert_eq!(
            install_source(&mut conn, "com.example.alpha"),
            Some(repo_id)
        );
        assert_eq!(repo_name(&mut conn, repo_id), "Test Repo");

        remove_install(&mut conn, "com.example.alpha").unwrap();
        assert!(install_source(&mut conn, "com.example.alpha").is_none());
    }

    #[test]
    fn picks_requested_locale() {
        let (mut conn, repo_id) = setup("fr");
        let d = detail(&mut conn, repo_id, "com.example.alpha")
            .unwrap()
            .unwrap();
        assert_eq!(d.name, "Alpha FR");
        assert_eq!(d.summary, "Résumé Alpha");
        assert_eq!(d.description, "Description Alpha");
        assert_eq!(d.anti_features[0].name, "Pistage"); // localized anti-feature
    }

    #[test]
    fn incremental_diff_merge_and_apply() {
        // The incremental-update core: merge the diff onto the base index (RFC 7386).
        let mut base: serde_json::Value = serde_json::from_str(BASE).unwrap();
        let patch: serde_json::Value = serde_json::from_str(DIFF).unwrap();
        crate::cache::merge_patch(&mut base, &patch);
        let merged: IndexV2 = serde_json::from_value(base).unwrap();
        assert_eq!(merged.repo.timestamp, 2000);
        assert!(merged.packages.contains_key("com.example.gamma"));
        assert!(!merged.packages.contains_key("com.example.beta"));

        // Apply base then the merged index to the DB; the incremental result must match.
        let mut conn = open_in_memory();
        let repo_id = ensure_repo(&mut conn, ADDR).unwrap();
        let base_index: IndexV2 = serde_json::from_str(BASE).unwrap();
        upsert_index(&mut conn, repo_id, &base_index, "en-US", false).unwrap();
        upsert_index(&mut conn, repo_id, &merged, "en-US", false).unwrap();

        assert!(
            detail(&mut conn, repo_id, "com.example.beta")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            detail(&mut conn, repo_id, "com.example.gamma")
                .unwrap()
                .unwrap()
                .author,
            "Carol"
        );
        assert_eq!(
            detail(&mut conn, repo_id, "com.example.alpha")
                .unwrap()
                .unwrap()
                .description,
            "Alpha UPDATED"
        );
        let r = repos(&mut conn).unwrap();
        assert_eq!(r[0].timestamp, 2000);
        assert_eq!(r[0].app_count, 2); // alpha + gamma
    }
}
