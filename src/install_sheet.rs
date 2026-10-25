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

fn human(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

/// The overlay, gated by the caller with `when(install.is_some, …)`.
pub fn install_sheet() -> impl Piece {
    let install = state::install_signal();

    let title = label(move || install.get().map(|u| u.name).unwrap_or_default())
        .font(Font::Title)
        .color(INK);

    let card = column((
        title,
        // One body per phase, rebuilt only when the phase changes (progress ticks keep the
        // downloading body and update it in place).
        keyed(
            move || install.get().map(|u| u.phase),
            move |phase| match phase {
                // Downloading: a determinate bar + byte counts + Cancel.
                InstallPhase::Downloading => column((
                    label(crate::res::str::install_downloading())
                        .font(Font::Subheadline)
                        .color(MUTED),
                    progress(move || install.get().map(|u| u.progress as f64).unwrap_or(0.0))
                        .id(day::id!(install_progress)),
                    label(move || {
                        install
                            .get()
                            .map(|u| {
                                crate::res::str::install_bytes(crate::res::str::InstallBytes {
                                    done: human(u.downloaded),
                                    total: human(u.total),
                                })
                                .format()
                            })
                            .unwrap_or_default()
                    })
                    .font(Font::Footnote)
                    .color(MUTED),
                    button(crate::res::str::btn_cancel())
                        .action(crate::install::cancel)
                        .id(day::id!(install_cancel)),
                ))
                .spacing(10.0)
                .align(HAlign::Leading)
                .any(),
                // Installing: the system confirm screen is up.
                InstallPhase::Installing => column((
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
                .any(),
                // Terminal states.
                InstallPhase::Success => column((
                    label(crate::res::str::install_done())
                        .font(Font::Headline)
                        .color(ACCENT),
                    dismiss_button(crate::res::str::btn_done(), true),
                ))
                .spacing(10.0)
                .align(HAlign::Leading)
                .any(),
                InstallPhase::Cancelled => column((
                    label(crate::res::str::install_cancelled())
                        .font(Font::Headline)
                        .color(MUTED),
                    dismiss_button(crate::res::str::btn_close(), false),
                ))
                .spacing(10.0)
                .align(HAlign::Leading)
                .any(),
                InstallPhase::Failed(message) => column((
                    label(crate::res::str::install_failed())
                        .font(Font::Headline)
                        .color(Color::hex(0xC0_39_2B)),
                    label(message.clone()).font(Font::Footnote).color(MUTED),
                    dismiss_button(crate::res::str::btn_close(), false),
                ))
                .spacing(10.0)
                .align(HAlign::Leading)
                .any(),
            },
        ),
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
}

fn dismiss_button<M>(text: impl IntoText<M>, prominent: bool) -> impl Piece {
    button(text)
        .apply_if(prominent, |b| b.prominent())
        .action(|| state::set_install(None))
        .id(day::id!(install_dismiss))
}
