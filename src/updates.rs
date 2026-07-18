//! The Updates tab: every installed app that App Fair's catalogs know about, with the ones that
//! have a newer version highlighted. Tapping a row opens the detail page (where Update lives).

use std::sync::atomic::{AtomicBool, Ordering};

use day::prelude::*;
use day_piece_remote_image::ContentMode;

use crate::model::{AppDetail, AppSummary};
use crate::state;
use crate::{Nav, push_nav};

/// Fires the opt-in auto-update batch at most once per launch, so a re-render doesn't restart it.
static AUTO_FIRED: AtomicBool = AtomicBool::new(false);

const MUTED: Color = Color::hex(0x6B_6B_70);
const PLACEHOLDER: Color = Color::hex(0xE3_E3_E8);
const UPDATE: Color = Color::hex(0x2F_6F_DE);
const OK: Color = Color::hex(0x2E7D32);

/// One installed app: its catalog summary + the on-device versionCode. `updatable` is derived.
#[derive(Clone)]
struct Installed {
    app: AppSummary,
    installed_code: i64,
}

impl Installed {
    fn updatable(&self) -> bool {
        self.app.version_code > self.installed_code
    }
}

fn rows() -> Vec<Installed> {
    let mut v: Vec<Installed> = state::installed_updates()
        .into_iter()
        .map(|(app, installed_code)| Installed {
            app,
            installed_code,
        })
        .collect();
    // Updatable first, then alphabetical.
    v.sort_by(|a, b| {
        b.updatable()
            .cmp(&a.updatable())
            .then_with(|| a.app.name.to_lowercase().cmp(&b.app.name.to_lowercase()))
    });
    v
}

/// The full detail record for every updatable app, for a batch update (#8).
fn updatable_details() -> Vec<AppDetail> {
    rows()
        .into_iter()
        .filter(Installed::updatable)
        .filter_map(|i| {
            state::with_db(|c| {
                crate::db::detail(c, i.app.repo_id, &i.app.pkg)
                    .ok()
                    .flatten()
            })
        })
        .collect()
}

pub fn updates_page(path: Signal<Vec<Nav>>) -> AnyPiece {
    let version = state::catalog_version();

    // When auto-download is on, kick off the batch update once per launch as soon as the catalog
    // reports updates and nothing is already installing (#8).
    watch(
        move || version.get(),
        move |_, _| {
            if crate::settings::auto_download()
                && !AUTO_FIRED.load(Ordering::Relaxed)
                && !state::install_active()
            {
                let apps = updatable_details();
                if !apps.is_empty() {
                    AUTO_FIRED.store(true, Ordering::Relaxed);
                    state::start_updates(apps);
                }
            }
        },
    );

    let summary = label(move || {
        version.get();
        let all = rows();
        let n_update = all.iter().filter(|i| i.updatable()).count();
        if all.is_empty() {
            crate::res::str::updates_none().format()
        } else if n_update == 0 {
            crate::res::str::updates_up_to_date(all.len() as i64).format()
        } else {
            crate::res::str::updates_available(n_update as i64).format()
        }
    })
    .font(Font::Footnote)
    .color(MUTED)
    .grow_w()
    .id("updates-summary");

    // "Update all" appears only while updates are pending; it installs them one after another.
    let update_all = when(
        move || {
            version.get();
            rows().iter().any(Installed::updatable)
        },
        move || {
            button(crate::res::str::btn_update_all())
                .prominent()
                .action(move || state::start_updates(updatable_details()))
                .id("update-all")
                .any()
        },
    );

    let header = row((summary, update_all.any()))
        .align(VAlign::Center)
        .spacing(8.0);

    let list = list(
        move || {
            version.get();
            rows()
        },
        |i: &Installed| i.app.pkg.clone(),
        update_row,
    )
    .row_height(RowHeight::Uniform(72.0))
    .on_select(move |pkg: String| {
        // Look up the repo id for this package to push its detail.
        if let Some(app) = state::with_db(|c| crate::db::summary_by_pkg(c, &pkg)) {
            push_nav(
                path,
                Nav::App {
                    repo_id: app.repo_id,
                    pkg,
                },
            );
        }
    })
    // id BEFORE grow so it lands on the list node (with the selection handler), not the wrapper.
    .id("updates-list")
    .grow();

    column((header, list))
        .spacing(10.0)
        .align(HAlign::Leading)
        .padding(12.0)
        .any()
}

fn update_row(slot: ItemSlot<Installed, String>) -> AnyPiece {
    let key = slot.key();

    let icon = crate::icons::row_icon(move || slot.field(|i| i.app.icon_url()))
        .rounded(10.0)
        .content_mode(ContentMode::Fit)
        .placeholder_color(PLACEHOLDER)
        .frame(48.0, 48.0);

    // Right column: an accent "Update →" line when a newer version exists, else "Up to date".
    // Two colored labels toggled reactively (label colors aren't reactive), so a recycled row's
    // status updates on rebind.
    let status = column((
        when(
            move || slot.with(|i| i.updatable()),
            move || {
                label(move || {
                    slot.with(|i| {
                        crate::res::str::status_update(i.app.version_name.clone()).format()
                    })
                })
                .font(Font::Caption)
                .color(UPDATE)
            },
        ),
        when(
            move || slot.with(|i| !i.updatable()),
            || {
                label(crate::res::str::status_up_to_date())
                    .font(Font::Caption)
                    .color(OK)
            },
        ),
    ));

    row((
        icon.any(),
        column((
            label(move || slot.field(|i| i.app.name.clone())).font(Font::Body),
            label(move || slot.field(|i| i.app.summary.clone()))
                .font(Font::Footnote)
                .color(MUTED),
        ))
        .spacing(2.0)
        .align(HAlign::Leading)
        .grow_w(),
        status,
    ))
    .spacing(12.0)
    .align(VAlign::Center)
    .padding(Insets::symmetric(6.0, 8.0))
    .id_keyed("update-row", key)
    .any()
}
