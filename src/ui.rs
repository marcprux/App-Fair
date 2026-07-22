//! The Catalogs tab: a catalog switcher, an "add catalog" button, native search, a category
//! filter, the sync status line, and the app list with icons. Tapping a row pushes the detail
//! page onto the tab's navigation stack.

use day::prelude::*;
use day_piece_remote_image::ContentMode;
use day_piece_searchfield::search_field;

use crate::model::{AppSummary, SortOrder};
use crate::state::{self, SyncUi};
use crate::{Nav, push_nav, sync};

const MUTED: Color = Color::hex(0x6B_6B_70);
const PLACEHOLDER: Color = Color::hex(0xE3_E3_E8);
const ACCENT: Color = Color::hex(0x2F_6F_DE);

pub fn catalogs_page(path: Signal<Vec<Nav>>) -> AnyPiece {
    let query = state::query();
    let category = state::category();
    let sort = state::sort_order();
    let version = state::catalog_version();

    let switcher = each(
        move || vec![version.get()],
        |v: &u64| *v,
        move |_| catalog_switcher(),
    );
    // Adding a custom F-Droid catalog is still a beta feature — the "Add" button is exposed only in
    // debug builds; release builds hide it and browse the default App Fair catalog alone.
    let mut header_items: Vec<AnyPiece> = vec![switcher.any().grow_w()];
    if cfg!(debug_assertions) {
        header_items.push(
            button(crate::res::str::btn_add())
                .action(move || push_nav(path, Nav::AddCatalog))
                .id("catalog-add")
                .any(),
        );
    }
    header_items.push(
        button(crate::res::str::btn_refresh())
            .action(sync::sync_all_enabled)
            .id("refresh")
            .any(),
    );
    let header = row(PieceVec(header_items))
        .align(VAlign::Center)
        .spacing(8.0);

    let search = search_field(query)
        .placeholder(crate::res::str::search_placeholder())
        .id("search");

    let category_region = each(
        move || vec![version.get()],
        |v: &u64| *v,
        move |_| category_picker(category),
    );
    // The category filter (which grows to fill) and the sort-order drop-down share one row.
    let controls = row((category_region.any().grow_w(), sort_picker(sort)))
        .spacing(8.0)
        .align(VAlign::Center);

    let sync_line = label(move || sync_text(&state::sync_signal().get()))
        .font(Font::Footnote)
        .color(MUTED)
        .id("sync-status");

    let list = list(
        move || {
            version.get();
            query.get();
            category.get();
            state::repo_filter().get();
            state::search_apps()
        },
        |a: &AppSummary| format!("{}~{}", a.repo_id, a.pkg),
        app_row,
    )
    .row_height(RowHeight::Uniform(72.0))
    .on_select(move |key: String| {
        if let Some((rid, pkg)) = key.split_once('~')
            && let Ok(rid) = rid.parse::<i64>()
        {
            push_nav(
                path,
                Nav::App {
                    repo_id: rid,
                    pkg: pkg.to_string(),
                },
            );
        }
    })
    // id BEFORE grow so it lands on the list node (which carries the selection handler), not the
    // grow wrapper — otherwise a scripted `select` can't reach the list.
    .id("app-list")
    .grow();

    column((header, search, controls, sync_line, list))
        .spacing(10.0)
        .align(HAlign::Leading)
        .padding(12.0)
        .any()
}

/// A native dropdown choosing the catalog list's sort order: Default (the catalog's `rank`), Last
/// Updated, or Name (locale-collated). Selecting one re-queries the list via [`state::search_apps`].
fn sort_picker(sort: Signal<SortOrder>) -> AnyPiece {
    let orders = [SortOrder::Default, SortOrder::LastUpdated, SortOrder::Name];
    let labels: Vec<String> = vec![
        crate::res::str::sort_default().format(),
        crate::res::str::sort_last_updated().format(),
        crate::res::str::sort_name().format(),
    ];

    let current = sort.get_untracked();
    let start = orders.iter().position(|o| *o == current).unwrap_or(0);
    let sel = Signal::new(start);

    watch(
        move || sel.get(),
        move |idx: &usize, _| {
            let chosen = orders.get(*idx).copied().unwrap_or_default();
            if sort.get_untracked() != chosen {
                sort.set(chosen);
            }
        },
    );

    picker(labels, sel).menu().id("sort-picker").any()
}

/// A native dropdown of "All Catalogs" + each enabled catalog. Selecting one filters the list.
fn catalog_switcher() -> AnyPiece {
    let enabled: Vec<_> = state::repos().into_iter().filter(|r| r.enabled).collect();
    let repo_filter = state::repo_filter();

    let ids: Vec<Option<i64>> = std::iter::once(None)
        .chain(enabled.iter().map(|r| Some(r.id)))
        .collect();
    let labels: Vec<String> = std::iter::once(crate::res::str::all_catalogs().format())
        .chain(enabled.iter().map(|r| {
            let name = if r.name.is_empty() {
                r.address.clone()
            } else {
                r.name.clone()
            };
            crate::res::str::catalog_option(r.app_count, name).format()
        }))
        .collect();

    let current = repo_filter.get_untracked();
    let start = ids.iter().position(|k| *k == current).unwrap_or(0);
    let sel = Signal::new(start);

    let ids_for_watch = ids.clone();
    watch(
        move || sel.get(),
        move |idx: &usize, _| {
            let chosen = ids_for_watch.get(*idx).copied().flatten();
            if repo_filter.get_untracked() != chosen {
                repo_filter.set(chosen);
            }
        },
    );

    picker(labels, sel).menu().id("catalog-switcher").any()
}

/// A native dropdown of "All" + every category. Selecting one filters the list.
fn category_picker(category: Signal<Option<String>>) -> AnyPiece {
    let cats = state::all_categories();
    if cats.is_empty() {
        return label("").any();
    }
    let keys: Vec<Option<String>> = std::iter::once(None)
        .chain(cats.iter().map(|c| Some(c.key.clone())))
        .collect();
    let labels: Vec<String> = std::iter::once(crate::res::str::all_categories().format())
        .chain(cats.iter().map(|c| c.name.clone()))
        .collect();

    let current = category.get_untracked();
    let start = keys.iter().position(|k| *k == current).unwrap_or(0);
    let sel = Signal::new(start);

    let keys_for_watch = keys.clone();
    watch(
        move || sel.get(),
        move |idx: &usize, _| {
            let chosen = keys_for_watch.get(*idx).cloned().flatten();
            if category.get_untracked() != chosen {
                category.set(chosen);
            }
        },
    );

    picker(labels, sel).menu().id("category-picker").any()
}

/// A recycling list row. Every field is read INSIDE a reactive closure, so a recycled cell
/// updates when it rebinds to a new app.
pub(crate) fn app_row(slot: ItemSlot<AppSummary, String>) -> AnyPiece {
    let key = slot.key();

    let icon = crate::icons::row_icon(move || slot.field(|a| a.icon_url()))
        .rounded(10.0)
        .content_mode(ContentMode::Fit)
        .placeholder_color(PLACEHOLDER)
        .frame(48.0, 48.0);

    row((
        icon.any(),
        column((
            label(move || slot.field(|a| a.name.clone())).font(Font::Body),
            label(move || slot.field(|a| a.summary.clone()))
                .font(Font::Footnote)
                .color(MUTED),
        ))
        .spacing(2.0)
        .align(HAlign::Leading)
        .grow_w(),
        label(move || slot.field(|a| a.version_name.clone()))
            .font(Font::Caption)
            .color(ACCENT),
    ))
    .spacing(12.0)
    .align(VAlign::Center)
    .padding(Insets::symmetric(6.0, 8.0))
    .id_keyed("app-row", key)
    .any()
}

fn sync_text(sync: &SyncUi) -> String {
    match sync {
        SyncUi::Idle => String::new(),
        SyncUi::Checking => crate::res::str::sync_checking().format(),
        SyncUi::Downloading(Some(f)) => {
            crate::res::str::sync_downloading_pct((f * 100.0) as i64).format()
        }
        SyncUi::Downloading(None) => crate::res::str::sync_downloading().format(),
        SyncUi::Building => crate::res::str::sync_building().format(),
        SyncUi::UpToDate => crate::res::str::sync_up_to_date().format(),
        SyncUi::Done(0) => crate::res::str::sync_up_to_date().format(),
        SyncUi::Done(n) => crate::res::str::sync_updated(*n as i64).format(),
        SyncUi::Error(e) => crate::res::str::sync_failed(e.as_str()).format(),
    }
}
