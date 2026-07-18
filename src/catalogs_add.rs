//! The "add a catalog" page (pushed from the Catalogs tab). Enter a F-Droid-compatible Index V2
//! URL (with its `?fingerprint=`), or pick a known repository, then App Fair adds it and syncs it.
//! Catalog URLs must be `https://` (or a `.onion`), and the fingerprint is pinned so the repo's
//! signed index can be verified (#1, #4).

use day::prelude::*;

use crate::model::split_index_url;
use crate::{db, state, sync};

const MUTED: Color = Color::hex(0x6B_6B_70);
const DANGER: Color = Color::hex(0xC6_28_28);

/// Known repositories, each with its published signing-cert fingerprint pinned via `?fingerprint=`.
const PRESETS: &[(&str, &str)] = &[
    (
        "Guardian Project",
        "https://guardianproject.info/fdroid/repo?fingerprint=B7C2EEFD8DAC7806AF67DFCD92EB18126BC08312A7F2D6F3862E46013C7A6135",
    ),
    (
        "IzzyOnDroid",
        "https://apt.izzysoft.de/fdroid/repo?fingerprint=3BF0D6ABFEAE2F401707B6D966BE743BF0EEE49C2561B9BA39073711F628937A",
    ),
    (
        "F-Droid Archive",
        "https://f-droid.org/archive?fingerprint=43238D512C1E5EB2D6569F4A3AFBF5523418B82E0A3ED1552770ABB9A9C9CCAB",
    ),
];

/// Split `?fingerprint=…` off a repo URL, returning `(url_without_query, fingerprint)`.
fn extract_fingerprint(url: &str) -> (String, String) {
    match url.split_once('?') {
        Some((base, query)) => {
            let fp = query
                .split('&')
                .find_map(|p| p.strip_prefix("fingerprint="))
                .unwrap_or("")
                .to_string();
            (base.to_string(), fp)
        }
        None => (url.to_string(), String::new()),
    }
}

/// A catalog URL must be `https://` — or `http://` only for a Tor `.onion` service.
fn is_secure(url: &str) -> bool {
    if url.starts_with("https://") {
        return true;
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        return host.ends_with(".onion");
    }
    false
}

/// Add the catalog at `raw` and start syncing it, then return to the Catalogs list. On a bad URL,
/// set `error` and stay.
fn add(raw: &str, error: Signal<String>) {
    let raw = raw.trim();
    let (clean, fingerprint) = extract_fingerprint(raw);
    if !is_secure(&clean) {
        error.set(crate::res::str::add_error_insecure().format());
        return;
    }
    let (base, file) = split_index_url(&clean);
    state::with_db(|conn| {
        if let Ok(id) = db::ensure_repo(conn, &base)
            && !fingerprint.is_empty()
        {
            let _ = db::set_repo_fingerprint(conn, id, &fingerprint.to_lowercase());
        }
    });
    state::bump_catalog();
    sync::start(base, file);
    nav_back();
}

pub fn add_catalog_page() -> AnyPiece {
    let url = Signal::new(String::new());
    let error = Signal::new(String::new());

    let presets: Vec<AnyPiece> = PRESETS
        .iter()
        .map(|(name, u)| {
            let u = *u;
            button(*name)
                .action(move || url.set(u.to_string()))
                .id_keyed("preset", *name)
                .any()
        })
        .collect();

    scroll(
        column((
            label(crate::res::str::add_title()).font(Font::Title),
            label(crate::res::str::add_blurb())
                .font(Font::Footnote)
                .color(MUTED),
            text_field(url)
                .placeholder(crate::res::str::add_placeholder())
                .id("catalog-url"),
            when(
                move || !error.get().is_empty(),
                move || {
                    label(move || error.get())
                        .font(Font::Footnote)
                        .color(DANGER)
                        .any()
                },
            )
            .any(),
            button(crate::res::str::btn_add_catalog())
                .prominent()
                .action(move || add(&url.get_untracked(), error))
                .id("catalog-confirm"),
            label(crate::res::str::add_known()).font(Font::Headline),
            column(PieceVec(presets))
                .spacing(8.0)
                .align(HAlign::Leading),
        ))
        .spacing(14.0)
        .align(HAlign::Leading)
        .padding(16.0),
    )
    .any()
}
