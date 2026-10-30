use gpui::{App, Global, Hsla, WindowAppearance, hsla};

use crate::platform;

pub struct Theme {
    /// Tint over the blurred window background.
    pub background: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub row_selected: Hsla,
    pub accent: Hsla,
    pub selection: Hsla,
    pub error: Hsla,
    /// Opaque background for regular windows such as settings.
    pub surface: Hsla,
    /// Background of text fields and buttons.
    pub field: Hsla,
}

impl Global for Theme {}

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        let mut theme = Self::base(appearance);
        if !platform::HAS_BACKDROP {
            theme.background.a = 0.96;
        }
        theme
    }

    fn base(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self {
                background: hsla(240. / 360., 0.06, 0.11, 0.25),
                border: hsla(0., 0., 1., 0.08),
                text: hsla(0., 0., 0.96, 1.),
                text_muted: hsla(0., 0., 0.96, 0.45),
                row_selected: hsla(0., 0., 1., 0.10),
                accent: hsla(212. / 360., 1., 0.62, 1.),
                selection: hsla(212. / 360., 1., 0.62, 0.35),
                error: hsla(2. / 360., 0.9, 0.68, 1.),
                surface: hsla(240. / 360., 0.06, 0.12, 1.),
                field: hsla(0., 0., 1., 0.07),
            },
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self {
                background: hsla(0., 0., 0.98, 0.25),
                border: hsla(0., 0., 0., 0.08),
                text: hsla(0., 0., 0.08, 1.),
                text_muted: hsla(0., 0., 0.08, 0.45),
                row_selected: hsla(0., 0., 0., 0.07),
                accent: hsla(212. / 360., 1., 0.48, 1.),
                selection: hsla(212. / 360., 1., 0.48, 0.25),
                error: hsla(2. / 360., 0.75, 0.48, 1.),
                surface: hsla(0., 0., 0.97, 1.),
                field: hsla(0., 0., 0., 0.05),
            },
        }
    }

    pub fn init(cx: &mut App) {
        cx.set_global(Self::for_appearance(cx.window_appearance()));
    }
}
