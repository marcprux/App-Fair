-- P0/P1/P2 store features: repo signing fingerprint + priority, per-app expected APK signer, and
-- an install-provenance table (which apps App Fair installed).

ALTER TABLE repos ADD COLUMN fingerprint TEXT NOT NULL DEFAULT '';
ALTER TABLE repos ADD COLUMN priority BIGINT NOT NULL DEFAULT 0;
-- Tab-separated mirror base URLs from the repo's index, tried when the primary host fails (#12).
ALTER TABLE repos ADD COLUMN mirrors TEXT NOT NULL DEFAULT '';

ALTER TABLE apps ADD COLUMN signer TEXT NOT NULL DEFAULT '';

CREATE TABLE installs (
    pkg          TEXT PRIMARY KEY,
    repo_id      BIGINT NOT NULL DEFAULT 0,
    installed_at BIGINT NOT NULL DEFAULT 0
);

-- The `apps` primary key is (repo_id, pkg), so a lookup by pkg alone can't use it. Cross-catalog
-- dedup (#6) and same-package lookups (updates, summary_by_pkg) correlate on pkg across all repos;
-- index it so those don't scan the whole table per row.
CREATE INDEX idx_apps_pkg ON apps (pkg);
