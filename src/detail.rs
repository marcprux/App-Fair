//! The app detail page: icon, name, the install/update/launch action, a device-compatibility
//! badge, a horizontal screenshot filmstrip, the (HTML-flattened) description and "what's new",
//! and the styled, tappable permissions and anti-features.

use day::prelude::*;
use day_piece_remote_image::{ContentMode, remote_image};

use crate::model::AppDetail;
use crate::{db, icons, platform, state};

const MUTED: Color = Color::hex(0x6B_6B_70);
const TEXT: Color = Color::hex(0x1B_1B_20);
const WARN: Color = Color::hex(0xB4_6A_00);
const ACCENT: Color = Color::hex(0x2F_6F_DE);
const OK: Color = Color::hex(0x2E7D32);
const DANGER: Color = Color::hex(0xC6_28_28);
/// Card fills behind a tappable permission (neutral) / anti-feature (warm) row.
const CARD: Color = Color::hex(0xF2_F3_F7);
const CARD_WARN: Color = Color::hex(0xFB_EF_DE);

pub fn detail_page(repo_id: i64, pkg: String) -> AnyPiece {
    let Some(app) = state::with_db(|conn| db::detail(conn, repo_id, &pkg).ok().flatten()) else {
        return column((label(crate::res::str::app_not_found()).font(Font::Title),))
            .padding(16.0)
            .any();
    };

    // Compatibility is fixed for a given device + app version, so compute it once. `min_sdk == 0`
    // (repo didn't declare one) or `device_sdk == 0` (unknown/off-device) → don't block.
    let device_sdk = platform::device_sdk();
    let compatible = app.min_sdk == 0 || device_sdk == 0 || app.min_sdk <= device_sdk;

    scroll(
        column((
            header(&app, compatible),
            compatibility(&app, device_sdk),
            manage(&app),
            screenshots(&app),
            description(&app),
            whats_new(&app),
            permissions(&app),
            anti_features(&app),
            links(&app),
        ))
        .spacing(18.0)
        .align(HAlign::Leading)
        .padding(16.0),
    )
    .any()
}

fn header(app: &AppDetail, compatible: bool) -> AnyPiece {
    let icon: AnyPiece = match app.icon_url() {
        Some(url) => remote_image(icons::image_signal(&url))
            .rounded(14.0)
            .content_mode(ContentMode::Fit)
            .frame(64.0, 64.0)
            .any(),
        None => rounded_rectangle(14.0)
            .fill(Color::hex(0xE3_E3_E8))
            .frame(64.0, 64.0)
            .any(),
    };

    let name = app.name.clone();
    let version = app.version_name.clone();

    // Author line: a link to the author's site when the catalog provides one, else plain text.
    let author_line: AnyPiece = if app.author.is_empty() {
        label(crate::res::str::unknown_author())
            .font(Font::Footnote)
            .color(MUTED)
            .any()
    } else if app.author_website.is_empty() {
        label(app.author.clone())
            .font(Font::Footnote)
            .color(MUTED)
            .any()
    } else {
        link(app.author.clone(), app.author_website.clone())
            .font(Font::Footnote)
            .color(ACCENT)
            .any()
    };

    // "Version 3.4.1 · 11 MB" — append the download size when the catalog reports one.
    let size = format_size(app.apk_size);
    let version_line = if size.is_empty() {
        crate::res::str::version_line(version)
    } else {
        crate::res::str::version_line_size(size, version)
    };

    // Rebuild the action button whenever the catalog changes — a successful install/update bumps
    // it, so the button flips (Install → Installed/Launch) without re-navigating.
    let app_for_button = app.clone();
    let button = each(
        move || vec![state::catalog_version().get()],
        |v: &u64| *v,
        move |_| action_button(&app_for_button, compatible),
    );

    row((
        icon,
        // The metadata column takes the width between the icon and the button, so a long title
        // wraps here instead of pushing the action button off the right edge.
        column((
            label(name).font(Font::Headline),
            author_line,
            label(version_line).font(Font::Footnote).color(MUTED),
        ))
        .spacing(3.0)
        .align(HAlign::Leading)
        .grow_w(),
        button.any(),
    ))
    .spacing(14.0)
    .align(VAlign::Center)
    .any()
}

/// Install / Update / Launch, based on the on-device version and device compatibility.
fn action_button(app: &AppDetail, compatible: bool) -> AnyPiece {
    match platform::installed_version(&app.pkg) {
        // Installed and current → Launch (already on the device, so compatibility is moot).
        Some(v) if v >= app.version_code => launch_button(app),
        Some(_) => install_button(crate::res::str::btn_update(), app, compatible),
        None => install_button(crate::res::str::btn_install(), app, compatible),
    }
}

fn launch_button(app: &AppDetail) -> AnyPiece {
    let (pkg, name) = (app.pkg.clone(), app.name.clone());
    button(crate::res::str::btn_launch())
        .prominent()
        .action(move || {
            let (pkg, name) = (pkg.clone(), name.clone());
            day::task(async move {
                let ok = confirm(crate::res::str::launch_confirm(name.clone()))
                    .confirm_label(crate::res::str::btn_launch())
                    .present()
                    .await;
                if ok {
                    // Report a launch failure instead of silently doing nothing.
                    if let Err(e) = platform::launch_app(&pkg) {
                        alert(crate::res::str::launch_fail_title(name))
                            .message(e)
                            .button(crate::res::str::btn_ok(), ())
                            .present()
                            .await;
                    }
                }
            });
        })
        .id("detail-launch")
        .any()
}

/// A prominent Install/Update button when the app can run here; otherwise a secondary button that
/// explains the incompatibility and never starts an install.
fn install_button<M>(verb: impl IntoText<M>, app: &AppDetail, compatible: bool) -> AnyPiece {
    if compatible {
        let app = app.clone();
        return button(verb)
            .prominent()
            .action(move || crate::install::start(&app))
            .id("detail-install")
            .any();
    }
    let (name, min_sdk, device_sdk) = (app.name.clone(), app.min_sdk, platform::device_sdk());
    button(verb)
        .action(move || {
            let (name, min_sdk, device_sdk) = (name.clone(), min_sdk, device_sdk);
            day::task(async move {
                alert(crate::res::str::incompatible_title(name))
                    .message(incompatible_message(min_sdk, device_sdk))
                    .button(crate::res::str::btn_ok(), ())
                    .present()
                    .await;
            });
        })
        .id("detail-install")
        .any()
}

/// A one-line device-compatibility badge under the header. Hidden when the SDK levels are unknown.
fn compatibility(app: &AppDetail, device_sdk: i64) -> AnyPiece {
    if device_sdk == 0 || app.min_sdk == 0 {
        return label("").any();
    }
    let compatible = app.min_sdk <= device_sdk;
    let (icon, text, color) = if compatible {
        (
            crate::res::images::ic_ok,
            crate::res::str::compatible_yes().format(),
            OK,
        )
    } else {
        (
            crate::res::images::ic_incompat,
            crate::res::str::compatible_needs(android_release(app.min_sdk)).format(),
            DANGER,
        )
    };
    row((
        image(icon).frame(20.0, 20.0).any(),
        label(text).font(Font::Footnote).color(color).grow_w(),
    ))
    .spacing(8.0)
    .align(VAlign::Center)
    .any()
}

/// Source + uninstall controls, shown only when App Fair installed this app and it's still on the
/// device (#9). Rebuilds on catalog changes so it disappears once the app is uninstalled.
fn manage(app: &AppDetail) -> AnyPiece {
    let app = app.clone();
    each(
        move || vec![state::catalog_version().get()],
        |v: &u64| *v,
        move |_| manage_body(&app),
    )
    .any()
}

fn manage_body(app: &AppDetail) -> AnyPiece {
    // Only when App Fair has a record of installing this app and it's still on the device.
    let Some(repo_id) = state::with_db(|conn| db::install_source(conn, &app.pkg)) else {
        return label("").any();
    };
    if platform::installed_version(&app.pkg).is_none() {
        return label("").any();
    }
    let source = state::with_db(|conn| db::repo_name(conn, repo_id));
    let source_text = if source.is_empty() {
        crate::res::str::installed_by_app_fair()
    } else {
        crate::res::str::installed_from(source)
    };

    let (pkg, name) = (app.pkg.clone(), app.name.clone());
    let uninstall = button(crate::res::str::btn_uninstall()).action(move || {
        let (pkg, name) = (pkg.clone(), name.clone());
        day::task(async move {
            let ok = confirm(crate::res::str::uninstall_confirm(name.clone()))
                .confirm_label(crate::res::str::btn_uninstall())
                .present()
                .await;
            if ok {
                match platform::uninstall(&pkg) {
                    Ok(()) => {
                        // Hand ownership back to the user: forget our record and let the button and
                        // this section refresh (the app leaves the device after the system prompt).
                        state::with_db(|conn| {
                            let _ = db::remove_install(conn, &pkg);
                        });
                        state::bump_installed();
                        state::bump_catalog();
                    }
                    Err(e) => {
                        alert(crate::res::str::uninstall_fail_title(name))
                            .message(e)
                            .button(crate::res::str::btn_ok(), ())
                            .present()
                            .await;
                    }
                }
            }
        });
    });

    row((
        label(source_text)
            .font(Font::Footnote)
            .color(MUTED)
            .grow_w(),
        uninstall.id("detail-uninstall"),
    ))
    .spacing(12.0)
    .align(VAlign::Center)
    .any()
}

/// The blocked-install dialog body.
fn incompatible_message(min_sdk: i64, device_sdk: i64) -> String {
    crate::res::str::incompatible_body(android_release(device_sdk), android_release(min_sdk))
        .format()
}

/// Marketing version + API level for common Android releases, e.g. `24` → `7.0 (API 24)`.
fn android_release(sdk: i64) -> String {
    let name = match sdk {
        21 => "5.0",
        22 => "5.1",
        23 => "6.0",
        24 => "7.0",
        25 => "7.1",
        26 => "8.0",
        27 => "8.1",
        28 => "9",
        29 => "10",
        30 => "11",
        31 => "12",
        32 => "12L",
        33 => "13",
        34 => "14",
        35 => "15",
        36 => "16",
        _ => return format!("API {sdk}"),
    };
    format!("{name} (API {sdk})")
}

/// Human-readable APK size, e.g. `820 KB`, `11 MB`, `1.2 GB`. Empty for a missing/zero size. Uses
/// binary units (1024) like most file managers; one decimal only when it adds information (`55 MB`,
/// not `55.0 MB`). The decimal point isn't locale-adapted — a device locale + CLDR would be needed,
/// which isn't wired up here (the catalog itself is fetched as `en-US`).
fn format_size(bytes: i64) -> String {
    if bytes <= 0 {
        return String::new();
    }
    let b = bytes as f64;
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let (value, unit) = if b >= GB {
        (b / GB, "GB")
    } else if b >= MB {
        (b / MB, "MB")
    } else if b >= KB {
        (b / KB, "KB")
    } else {
        return format!("{bytes} B");
    };
    // Round to one decimal, then drop a trailing `.0`.
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded.fract()).abs() < 0.05 {
        format!("{rounded:.0} {unit}")
    } else {
        format!("{rounded:.1} {unit}")
    }
}

fn screenshots(app: &AppDetail) -> AnyPiece {
    let urls = app.screenshot_urls();
    if urls.is_empty() {
        return label("").any();
    }
    // A horizontal filmstrip of screenshot cards.
    let shots: Vec<AnyPiece> = urls
        .iter()
        .take(12)
        .map(|url| {
            remote_image(icons::image_signal(url))
                .rounded(12.0)
                .content_mode(ContentMode::Fit)
                .frame(190.0, 340.0)
                .any()
        })
        .collect();
    section_block(
        crate::res::str::section_screenshots(),
        scroll(row(PieceVec(shots)).spacing(12.0).align(VAlign::Top))
            .horizontal()
            .height(340.0)
            .any(),
    )
}

fn description(app: &AppDetail) -> AnyPiece {
    let raw = if app.description.is_empty() {
        &app.summary
    } else {
        &app.description
    };
    if raw.is_empty() {
        return label("").any();
    }
    // Catalogs put light HTML (<b>, <i>, <a>, lists) in descriptions. Flatten it to plain text.
    // TODO(styled-text): once Day labels support attributed/styled runs, render this as styled
    // text and keep the emphasis and links (see crate::util::strip_html).
    let text = crate::util::strip_html(raw);
    if text.is_empty() {
        return label("").any();
    }
    column((label(text).font(Font::Body),))
        .align(HAlign::Leading)
        .any()
}

fn whats_new(app: &AppDetail) -> AnyPiece {
    if app.whats_new.trim().is_empty() {
        return label("").any();
    }
    section_block(
        crate::res::str::section_whats_new(),
        label(crate::util::strip_html(&app.whats_new))
            .font(Font::Body)
            .any(),
    )
}

fn permissions(app: &AppDetail) -> AnyPiece {
    if app.permissions.is_empty() {
        return label("").any();
    }
    let rows: Vec<AnyPiece> = app
        .permissions
        .iter()
        .map(|p| {
            let (title, desc) = platform::permission_info(p);
            let raw = p.clone();
            let dialog_title = title.clone();
            info_row(crate::res::images::ic_perm, title, TEXT, CARD, move || {
                let (dialog_title, desc, raw) = (dialog_title.clone(), desc.clone(), raw.clone());
                day::task(async move {
                    let sys = crate::res::str::perm_system_label().format();
                    let body = if desc.is_empty() {
                        format!("{sys}\n{raw}")
                    } else {
                        format!("{desc}\n\n{sys}\n{raw}")
                    };
                    alert(dialog_title)
                        .message(body)
                        .button(crate::res::str::btn_ok(), ())
                        .present()
                        .await;
                });
            })
        })
        .collect();
    section_block(
        crate::res::str::section_permissions(),
        column(PieceVec(rows))
            .align(HAlign::Leading)
            .spacing(8.0)
            .any(),
    )
}

fn anti_features(app: &AppDetail) -> AnyPiece {
    if app.anti_features.is_empty() {
        return label("").any();
    }
    let rows: Vec<AnyPiece> = app
        .anti_features
        .iter()
        .map(|af| {
            let (name, desc) = (af.name.clone(), af.description.clone());
            let dialog_title = name.clone();
            info_row(
                crate::res::images::ic_anti,
                name,
                WARN,
                CARD_WARN,
                move || {
                    let (dialog_title, desc) = (dialog_title.clone(), desc.clone());
                    day::task(async move {
                        let body = if desc.is_empty() {
                            crate::res::str::antifeature_no_desc().format()
                        } else {
                            desc
                        };
                        alert(dialog_title)
                            .message(body)
                            .button(crate::res::str::btn_ok(), ())
                            .present()
                            .await;
                    });
                },
            )
        })
        .collect();
    section_block(
        crate::res::str::section_antifeatures(),
        column(PieceVec(rows))
            .align(HAlign::Leading)
            .spacing(8.0)
            .any(),
    )
}

/// One styled, tappable row: a leading category icon, the title, and a trailing info glyph that
/// signals the row opens a details dialog. `card` is the rounded background fill.
fn info_row(
    icon: impl Into<day::ImageName>,
    title: String,
    title_color: Color,
    card: Color,
    on_tap: impl Fn() + 'static,
) -> AnyPiece {
    row((
        image(icon).frame(24.0, 24.0).any(),
        label(title).font(Font::Body).color(title_color).grow_w(),
        image(crate::res::images::ic_info).frame(18.0, 18.0).any(),
    ))
    .spacing(12.0)
    .align(VAlign::Center)
    .padding(Insets::symmetric(12.0, 10.0))
    .background(card)
    .corner_radius(12.0)
    .on_tap(on_tap)
}

fn links(app: &AppDetail) -> AnyPiece {
    let mut rows: Vec<AnyPiece> = Vec::new();
    if !app.license.is_empty() {
        rows.push(
            label(crate::res::str::license_line(app.license.clone()))
                .font(Font::Footnote)
                .color(MUTED)
                .any(),
        );
    }
    // Website and Source are URLs — render them as links that open the system browser.
    if !app.website.is_empty() {
        rows.push(
            link(
                crate::res::str::website_line(app.website.clone()),
                app.website.clone(),
            )
            .font(Font::Footnote)
            .color(ACCENT)
            .any(),
        );
    }
    if !app.source_code.is_empty() {
        rows.push(
            link(
                crate::res::str::source_line(app.source_code.clone()),
                app.source_code.clone(),
            )
            .font(Font::Footnote)
            .color(ACCENT)
            .any(),
        );
    }
    rows.push(
        label(crate::res::str::package_line(app.pkg.clone()))
            .font(Font::Footnote)
            .color(MUTED)
            .any(),
    );
    // The pinned signing key (#3): its first bytes are enough for a user to recognize the signer
    // across updates without a wall of hex.
    if !app.signer.is_empty() {
        rows.push(
            label(crate::res::str::signed_by(short_fingerprint(&app.signer)))
                .font(Font::Footnote)
                .color(MUTED)
                .any(),
        );
    }
    column(PieceVec(rows))
        .align(HAlign::Leading)
        .spacing(3.0)
        .any()
}

/// A recognizable prefix of a signing-cert SHA-256, grouped in colon-separated byte pairs, e.g.
/// `43:23:8d:51:2c:1e:5e:b2…`. Full comparison isn't a human task; recognition is.
fn short_fingerprint(fp: &str) -> String {
    let head: String = fp.chars().take(16).collect();
    let grouped = head
        .as_bytes()
        .chunks(2)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(":");
    if fp.chars().count() > 16 {
        format!("{grouped}…")
    } else {
        grouped
    }
}

fn section_block<M>(title: impl IntoText<M>, body: AnyPiece) -> AnyPiece {
    column((label(title).font(Font::Headline), body))
        .spacing(8.0)
        .align(HAlign::Leading)
        .any()
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn formats_apk_sizes() {
        assert_eq!(format_size(0), "");
        assert_eq!(format_size(-5), "");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(11 * 1024 * 1024), "11 MB");
        // 1.5 MiB keeps its decimal; a whole number drops it.
        assert_eq!(format_size(1024 * 1024 + 512 * 1024), "1.5 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1 GB");
        assert_eq!(
            format_size(1024 * 1024 * 1024 + 200 * 1024 * 1024),
            "1.2 GB"
        );
    }
}
