//! The Settings tab: manage catalogs (enable/remove), a launch-sync preference, clear the image
//! cache, and an About blurb.

use day::prelude::*;

use crate::model::RepoRow;
use crate::{db, state};

const MUTED: Color = Color::hex(0x6B_6B_70);

const PREF_SYNC_ON_LAUNCH: &str = "af_sync_on_launch";
const PREF_AUTO_DOWNLOAD: &str = "af_auto_download";
const PREF_EXCLUDE_ANTI: &str = "af_exclude_anti";

/// Anti-feature keys the store shows a toggle for, so most users don't want to see them (#13).
pub const FILTERABLE_ANTI: &[(&str, &str)] = &[
    ("Ads", "Advertising"),
    ("Tracking", "Tracks you"),
    ("NonFreeNet", "Non-free network services"),
    ("NonFreeAdd", "Non-free add-ons"),
    ("NonFreeDep", "Non-free dependencies"),
    ("UpstreamNonFree", "Non-free upstream"),
    ("NonFreeAssets", "Non-free assets"),
    ("KnownVuln", "Known vulnerability"),
    ("NoSourceSince", "No source since"),
];

/// Whether App Fair checks catalogs for updates on launch (default on).
pub fn sync_on_launch() -> bool {
    day_part_prefs::get(PREF_SYNC_ON_LAUNCH).as_deref() != Some("0")
}

/// Whether App Fair downloads available updates automatically (still confirms each install). Off by
/// default (#8).
pub fn auto_download() -> bool {
    day_part_prefs::get(PREF_AUTO_DOWNLOAD).as_deref() == Some("1")
}

/// Anti-feature keys the user chose to hide from browse/search (#13).
pub fn excluded_anti_features() -> Vec<String> {
    day_part_prefs::get(PREF_EXCLUDE_ANTI)
        .map(|s| {
            s.split(',')
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn set_anti_excluded(key: &str, excluded: bool) {
    let mut keys: Vec<String> = excluded_anti_features();
    keys.retain(|k| k != key);
    if excluded {
        keys.push(key.to_string());
    }
    day_part_prefs::set(PREF_EXCLUDE_ANTI, &keys.join(","));
}

/// The localized display label for a filterable anti-feature key (#16). The `FILTERABLE_ANTI` list
/// is fixed, so the fallback never fires in practice.
fn anti_label(key: &str) -> LocalizedText {
    match key {
        "Ads" => crate::res::str::af_ads(),
        "Tracking" => crate::res::str::af_tracking(),
        "NonFreeNet" => crate::res::str::af_nonfreenet(),
        "NonFreeAdd" => crate::res::str::af_nonfreeadd(),
        "NonFreeDep" => crate::res::str::af_nonfreedep(),
        "UpstreamNonFree" => crate::res::str::af_upstreamnonfree(),
        "NonFreeAssets" => crate::res::str::af_nonfreeassets(),
        "KnownVuln" => crate::res::str::af_knownvuln(),
        "NoSourceSince" => crate::res::str::af_nosourcesince(),
        _ => day::tr(key),
    }
}

pub fn settings_page() -> AnyPiece {
    let version = state::catalog_version();

    let repo_list = each(
        move || {
            version.get();
            state::repos()
        },
        |r: &RepoRow| r.id,
        repo_row,
    );

    let sync_pref = Signal::new(sync_on_launch());
    watch(
        move || sync_pref.get(),
        move |on: &bool, _| {
            day_part_prefs::set(PREF_SYNC_ON_LAUNCH, if *on { "1" } else { "0" });
        },
    );

    let auto_pref = Signal::new(auto_download());
    watch(
        move || auto_pref.get(),
        move |on: &bool, _| {
            day_part_prefs::set(PREF_AUTO_DOWNLOAD, if *on { "1" } else { "0" });
        },
    );

    // One toggle per filterable anti-feature; flipping one re-filters the browse/search lists.
    let excluded = excluded_anti_features();
    let anti_rows: Vec<AnyPiece> = FILTERABLE_ANTI
        .iter()
        .map(|(key, _text)| {
            let key = *key;
            let on = Signal::new(excluded.iter().any(|k| k == key));
            watch(
                move || on.get(),
                move |v: &bool, _| {
                    set_anti_excluded(key, *v);
                    state::bump_catalog();
                },
            );
            row((
                label(anti_label(key)).font(Font::Body).grow_w(),
                toggle(on).id_keyed("anti", key),
            ))
            .align(VAlign::Center)
            .spacing(10.0)
            .any()
        })
        .collect();

    let cleared = Signal::new(false);

    scroll(
        column((
            label(crate::res::str::settings_catalogs()).font(Font::Headline),
            label(crate::res::str::settings_catalogs_blurb())
                .font(Font::Footnote)
                .color(MUTED),
            repo_list.any(),
            divider(),
            label(crate::res::str::settings_preferences()).font(Font::Headline),
            row((
                column((
                    label(crate::res::str::pref_sync_title()).font(Font::Body),
                    label(crate::res::str::pref_sync_blurb())
                        .font(Font::Footnote)
                        .color(MUTED),
                ))
                .spacing(2.0)
                .align(HAlign::Leading)
                .grow_w(),
                toggle(sync_pref).id("pref-sync"),
            ))
            .align(VAlign::Center)
            .spacing(10.0),
            row((
                column((
                    label(crate::res::str::pref_auto_title()).font(Font::Body),
                    label(crate::res::str::pref_auto_blurb())
                        .font(Font::Footnote)
                        .color(MUTED),
                ))
                .spacing(2.0)
                .align(HAlign::Leading)
                .grow_w(),
                toggle(auto_pref).id("pref-auto-download"),
            ))
            .align(VAlign::Center)
            .spacing(10.0),
            row((
                column((
                    label(crate::res::str::clear_cache_title()).font(Font::Body),
                    label(move || {
                        if cleared.get() {
                            crate::res::str::clear_cache_done().format()
                        } else {
                            crate::res::str::clear_cache_blurb().format()
                        }
                    })
                    .font(Font::Footnote)
                    .color(MUTED),
                ))
                .spacing(2.0)
                .align(HAlign::Leading)
                .grow_w(),
                button(crate::res::str::btn_clear())
                    .action(move || {
                        let dir = crate::platform::data_dir().join("imgcache");
                        let _ = std::fs::remove_dir_all(&dir);
                        cleared.set(true);
                    })
                    .id("clear-cache"),
            ))
            .align(VAlign::Center)
            .spacing(10.0),
            divider(),
            column((
                label(crate::res::str::settings_hide()).font(Font::Headline),
                label(crate::res::str::settings_hide_blurb())
                    .font(Font::Footnote)
                    .color(MUTED),
                column(PieceVec(anti_rows))
                    .spacing(8.0)
                    .align(HAlign::Leading),
            ))
            .spacing(8.0)
            .align(HAlign::Leading),
            divider(),
            column((
                label(crate::res::str::settings_about()).font(Font::Headline),
                label(crate::res::str::about_version(env!("CARGO_PKG_VERSION"))).font(Font::Body),
                label(crate::res::str::about_blurb())
                    .font(Font::Footnote)
                    .color(MUTED),
                label(crate::res::str::about_privacy())
                    .font(Font::Footnote)
                    .color(MUTED),
            ))
            .spacing(6.0)
            .align(HAlign::Leading),
        ))
        .spacing(14.0)
        .align(HAlign::Leading)
        .padding(16.0),
    )
    .any()
}

fn repo_row(slot: ItemSlot<RepoRow, i64>) -> AnyPiece {
    let r = slot.get();
    let id = r.id;
    let address = r.address.clone();
    let title = if r.name.is_empty() {
        r.address.clone()
    } else {
        r.name.clone()
    };

    let enabled = Signal::new(r.enabled);
    watch(
        move || enabled.get(),
        move |on: &bool, _| {
            let on = *on;
            state::with_db(|conn| {
                let _ = db::set_repo_enabled(conn, id, on);
            });
            state::bump_catalog();
        },
    );

    row((
        column((
            label(title).font(Font::Body),
            label(crate::res::str::repo_row_meta(
                r.address.clone(),
                r.app_count,
            ))
            .font(Font::Caption)
            .color(MUTED),
        ))
        .spacing(2.0)
        .align(HAlign::Leading)
        .grow_w(),
        toggle(enabled).id_keyed("repo-enabled", id),
        button(crate::res::str::btn_remove())
            .action(move || {
                state::with_db(|conn| {
                    let _ = db::delete_repo(conn, id);
                });
                crate::cache::remove_index(&address);
                state::bump_catalog();
            })
            .id_keyed("repo-remove", id),
    ))
    .spacing(10.0)
    .align(VAlign::Center)
    .padding(Insets::symmetric(0.0, 6.0))
    .any()
}
