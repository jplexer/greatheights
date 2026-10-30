//! Window-system tweaks GPUI doesn't expose.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(target_os = "macos"))]
mod fallback {
    use gpui::{Context, Window, WindowBackgroundAppearance, px, size};

    /// Transparent windows vary by compositor, so stay opaque and square.
    pub const WINDOW_BACKGROUND: WindowBackgroundAppearance = WindowBackgroundAppearance::Opaque;

    pub const ROUNDED_PANEL: bool = false;

    pub fn init() {}

    /// GPUI can't hide a window here, so there is no native handle: the
    /// launcher closes the window on hide and reopens it on show.
    pub struct NativeWindow;

    impl NativeWindow {
        pub fn of(_window: &Window) -> Option<Self> {
            None
        }

        pub fn show(&self) {}

        pub fn hide(&self) {}

        pub fn configure_as_panel(&self) {}

        pub fn request_activation(&self) {}

        pub fn app_is_active(&self) -> bool {
            true
        }

        pub fn present(&self) {}
    }

    pub fn run_in_background() {}

    /// Other platforms already keep the top-left corner fixed on resize.
    pub fn set_content_height<T>(window: &mut Window, height: f32, _cx: &mut Context<T>) {
        let width = window.bounds().size.width;
        window.resize(size(width, px(height)));
    }
}
#[cfg(not(target_os = "macos"))]
pub use fallback::*;
