//! Diesel table definitions for the catalog database. Kept in sync by hand with `migrations/`
//! (the app is small enough not to need `diesel print-schema`).

diesel::table! {
    repos (id) {
        id -> BigInt,
        address -> Text,
        name -> Text,
        timestamp -> BigInt,
        enabled -> Bool,
        fingerprint -> Text,
        priority -> BigInt,
        mirrors -> Text,
    }
}

diesel::table! {
    apps (repo_id, pkg) {
        repo_id -> BigInt,
        pkg -> Text,
        name -> Text,
        summary -> Text,
        description -> Text,
        license -> Text,
        author -> Text,
        author_website -> Text,
        website -> Text,
        source_code -> Text,
        icon_path -> Text,
        categories -> Text,
        screenshots -> Text,
        last_updated -> BigInt,
        version_name -> Text,
        version_code -> BigInt,
        apk_path -> Text,
        apk_size -> BigInt,
        apk_sha256 -> Text,
        min_sdk -> BigInt,
        permissions -> Text,
        anti_features -> Text,
        whats_new -> Text,
        signer -> Text,
    }
}

diesel::table! {
    installs (pkg) {
        pkg -> Text,
        repo_id -> BigInt,
        installed_at -> BigInt,
    }
}

diesel::table! {
    categories (repo_id, key) {
        repo_id -> BigInt,
        key -> Text,
        name -> Text,
    }
}

diesel::table! {
    anti_features (repo_id, key) {
        repo_id -> BigInt,
        key -> Text,
        name -> Text,
        description -> Text,
    }
}

diesel::table! {
    meta (key) {
        key -> Text,
        value -> Text,
    }
}

diesel::joinable!(apps -> repos (repo_id));
diesel::joinable!(categories -> repos (repo_id));
diesel::joinable!(anti_features -> repos (repo_id));
diesel::allow_tables_to_appear_in_same_query!(apps, repos, categories, anti_features, meta);
