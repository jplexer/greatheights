//! Colors and fonts, after Zed's default One Dark and One Light themes.

use std::borrow::Cow;

use greatheights_core::Appearance;
use gpui::{App, Global, Hsla, Rgba, WindowAppearance, rgb, rgba};

use crate::platform;
use crate::window::Config;

/// UI text, as in Zed.
pub const UI_FONT: &str = "IBM Plex Sans";
/// Text you type, as in Zed's editors.
pub const MONO_FONT: &str = "Lilex";

pub struct Theme {
    /// Panels: the launcher and settings.
    pub background: Hsla,
    /// Text fields and buttons.
    pub field: Hsla,
    pub border: Hsla,
    /// Dividers inside a panel.
    pub border_variant: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_placeholder: Hsla,
    pub row_hover: Hsla,
    pub row_selected: Hsla,
    pub accent: Hsla,
    /// Selected text.
    pub selection: Hsla,
    pub error: Hsla,
}

impl Global for Theme {}

fn color(value: Rgba) -> Hsla {
    value.into()
}

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self {
                background: color(rgb(0x2f343e)),
                field: color(rgb(0x282c33)),
                border: color(rgb(0x464b57)),
                border_variant: color(rgb(0x363c46)),
                text: color(rgb(0xdce0e5)),
                text_muted: color(rgb(0xa9afbc)),
                text_placeholder: color(rgb(0x878a98)),
                row_hover: color(rgb(0x363c46)),
                row_selected: color(rgb(0x454a56)),
                accent: color(rgb(0x74ade8)),
                selection: color(rgba(0x74ade83d)),
                error: color(rgb(0xd07277)),
            },
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self {
                background: color(rgb(0xebebec)),
                field: color(rgb(0xfafafa)),
                border: color(rgb(0xc9c9ca)),
                border_variant: color(rgb(0xdfdfe0)),
                text: color(rgb(0x242529)),
                text_muted: color(rgb(0x58585a)),
                text_placeholder: color(rgb(0x7e8086)),
                row_hover: color(rgb(0xdfdfe0)),
                row_selected: color(rgb(0xd3d3d4)),
                accent: color(rgb(0x5c78e2)),
                selection: color(rgba(0x5c78e23d)),
                error: color(rgb(0xd36151)),
            },
        }
    }

    pub fn init(cx: &mut App) {
        let fonts = [
            include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf").as_slice(),
            include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf").as_slice(),
            include_bytes!("../assets/fonts/Lilex-Regular.ttf").as_slice(),
        ];
        if let Err(err) = cx
            .text_system()
            .add_fonts(fonts.into_iter().map(Cow::Borrowed).collect())
        {
            log::error!("couldn't load bundled fonts: {err}");
        }
        cx.set_global(Self::current(cx));
    }

    /// The theme for the chosen appearance, or the system's.
    pub fn current(cx: &App) -> Self {
        let appearance = cx.try_global::<Config>().map(|c| c.settings.appearance);
        Self::for_appearance(match appearance {
            Some(Appearance::Light) => WindowAppearance::Light,
            Some(Appearance::Dark) => WindowAppearance::Dark,
            Some(Appearance::System) | None => cx.window_appearance(),
        })
    }
}

/// Switches to the chosen appearance and repaints every window.
pub fn apply(appearance: Appearance, cx: &mut App) {
    // Changing the native appearance makes AppKit notify each window, which
    // re-enters GPUI, so it can't happen inside a window update.
    cx.spawn(async move |cx| {
        platform::set_appearance(appearance);
        let _ = cx.update(|cx| {
            cx.set_global(Theme::current(cx));
            cx.refresh_windows();
        });
    })
    .detach();
}
