//! The download/install overlay: a card over a dimmed backdrop with a progress bar while
//! downloading, a spinner while the system installer runs, and a result with a dismiss button.
//! It reads the install signal reactively, so progress and phase update in place.

use day::prelude::*;

use crate::state::{self, InstallPhase};

const SCRIM: Color = Color::rgba(0.0, 0.0, 0.0, 0.5);
const CARD: Color = Color::hex(0xFF_FF_FF);
const INK: Color = Color::hex(0x15_15_17);
const MUTED: Color = Color::hex(0x6B_6B_70);
const ACCENT: Color = Color::hex(0x2F_6F_DE);

fn phase() -> Option<InstallPhase> {
    state::install_signal().get().map(|u| u.phase)
}

fn is(p: InstallPhase) -> bool {
    phase().as_ref() == Some(&p)
}

fn is_downloading() -> bool {
    is(InstallPhase::Downloading)
}
fn is_installing() -> bool {
    is(InstallPhase::Installing)
}
fn is_success() -> bool {
    is(InstallPhase::Success)
}
fn is_cancelled() -> bool {
    is(InstallPhase::Cancelled)
}
fn is_failed() -> bool {
    matches!(phase(), Some(InstallPhase::Failed(_)))
}

fn human(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

/// The overlay, gated by the caller with `when(install.is_some, …)`.
pub fn install_sheet() -> AnyPiece {
    let install = state::install_signal();

    let title = label(move || install.get().map(|u| u.name).unwrap_or_default())
        .font(Font::Title)
        .color(INK);

    let card = column((
        title,
        // Downloading: a determinate bar + byte counts + Cancel.
        when(is_downloading, || {
            let install = state::install_signal();
            column((
                label(crate::res::str::install_downloading())
                    .font(Font::Subheadline)
                    .color(MUTED),
                progress(move || install.get().map(|u| u.progress as f64).unwrap_or(0.0))
                    .id("install-progress"),
                label(move || {
                    install
                        .get()
                        .map(|u| {
                            crate::res::str::install_bytes(human(u.downloaded), human(u.total))
                                .format()
                        })
                        .unwrap_or_default()
                })
                .font(Font::Footnote)
                .color(MUTED),
                button(crate::res::str::btn_cancel())
                    .action(crate::install::cancel)
                    .id("install-cancel"),
            ))
            .spacing(10.0)
            .align(HAlign::Leading)
        }),
        // Installing: the system confirm screen is up.
        when(is_installing, || {
            column((
                spinner(),
                label(crate::res::str::install_installing())
                    .font(Font::Subheadline)
                    .color(MUTED),
                label(crate::res::str::install_confirm_hint())
                    .font(Font::Footnote)
                    .color(MUTED),
            ))
            .spacing(10.0)
            .align(HAlign::Leading)
        }),
        // Terminal states.
        when(is_success, || {
            column((
                label(crate::res::str::install_done())
                    .font(Font::Headline)
                    .color(ACCENT),
                dismiss_button(crate::res::str::btn_done(), true),
            ))
            .spacing(10.0)
            .align(HAlign::Leading)
        }),
        when(is_cancelled, || {
            column((
                label(crate::res::str::install_cancelled())
                    .font(Font::Headline)
                    .color(MUTED),
                dismiss_button(crate::res::str::btn_close(), false),
            ))
            .spacing(10.0)
            .align(HAlign::Leading)
        }),
        when(is_failed, || {
            let install = state::install_signal();
            column((
                label(crate::res::str::install_failed())
                    .font(Font::Headline)
                    .color(Color::hex(0xC0_39_2B)),
                label(move || match install.get().map(|u| u.phase) {
                    Some(InstallPhase::Failed(m)) => m,
                    _ => String::new(),
                })
                .font(Font::Footnote)
                .color(MUTED),
                dismiss_button(crate::res::str::btn_close(), false),
            ))
            .spacing(10.0)
            .align(HAlign::Leading)
        }),
    ))
    .spacing(14.0)
    .align(HAlign::Leading)
    .padding(22.0)
    .background(CARD)
    .corner_radius(18.0)
    .width(320.0);

    // Center the card over a full-bleed dim backdrop.
    column((spacer(), row((spacer(), card, spacer())), spacer()))
        .grow()
        .background(SCRIM)
        .any()
}

fn dismiss_button<M>(text: impl IntoText<M>, prominent: bool) -> AnyPiece {
    if prominent {
        button(text)
            .prominent()
            .action(|| state::set_install(None))
            .id("install-dismiss")
            .any()
    } else {
        button(text)
            .action(|| state::set_install(None))
            .id("install-dismiss")
            .any()
    }
}
