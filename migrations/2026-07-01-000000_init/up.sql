-- Initial App Fair catalog schema. Diesel runs this once (tracked in
-- __diesel_schema_migrations); later schema changes are new migration directories.

CREATE TABLE repos (
    id        INTEGER PRIMARY KEY,
    address   TEXT NOT NULL UNIQUE,
    name      TEXT NOT NULL DEFAULT '',
    timestamp BIGINT NOT NULL DEFAULT 0,
    enabled   INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE apps (
    repo_id        INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    pkg            TEXT NOT NULL,
    name           TEXT NOT NULL DEFAULT '',
    summary        TEXT NOT NULL DEFAULT '',
    description    TEXT NOT NULL DEFAULT '',
    license        TEXT NOT NULL DEFAULT '',
    author         TEXT NOT NULL DEFAULT '',
    author_website TEXT NOT NULL DEFAULT '',
    website        TEXT NOT NULL DEFAULT '',
    source_code    TEXT NOT NULL DEFAULT '',
    icon_path      TEXT NOT NULL DEFAULT '',
    categories     TEXT NOT NULL DEFAULT '[]',
    screenshots    TEXT NOT NULL DEFAULT '[]',
    last_updated   BIGINT NOT NULL DEFAULT 0,
    version_name   TEXT NOT NULL DEFAULT '',
    version_code   BIGINT NOT NULL DEFAULT 0,
    apk_path       TEXT NOT NULL DEFAULT '',
    apk_size       BIGINT NOT NULL DEFAULT 0,
    apk_sha256     TEXT NOT NULL DEFAULT '',
    min_sdk        BIGINT NOT NULL DEFAULT 0,
    permissions    TEXT NOT NULL DEFAULT '[]',
    anti_features  TEXT NOT NULL DEFAULT '[]',
    whats_new      TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (repo_id, pkg)
);
CREATE INDEX apps_name ON apps(name);
CREATE INDEX apps_updated ON apps(last_updated DESC);

CREATE TABLE categories (
    repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    key     TEXT NOT NULL,
    name    TEXT NOT NULL,
    PRIMARY KEY (repo_id, key)
);

CREATE TABLE anti_features (
    repo_id     INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (repo_id, key)
);

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL DEFAULT ''
);
