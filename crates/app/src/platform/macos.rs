use greatheights_core::Appearance;
use gpui::{Context, Window, WindowBackgroundAppearance};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationActivationPolicy, NSEvent, NSScreen, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowButton,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub const WINDOW_BACKGROUND: WindowBackgroundAppearance = WindowBackgroundAppearance::Opaque;

/// macOS draws the panel's border and rounded corners itself.
pub const DRAW_BORDER: bool = false;

/// Runs as an accessory: no Dock icon, no menu bar.
pub fn init() {
    let mtm = MainThreadMarker::new().expect("init must run on the main thread");
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

/// Makes native parts (title bars, the file picker) match the chosen
/// appearance. GPUI reads the app's appearance too, so `System` must clear
/// it rather than set the current one.
pub fn set_appearance(appearance: Appearance) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let name = match appearance {
        Appearance::System => None,
        // SAFETY: AppKit's appearance names are immutable statics.
        Appearance::Light => Some(unsafe { NSAppearanceNameAqua }),
        Appearance::Dark => Some(unsafe { NSAppearanceNameDarkAqua }),
    };
    let appearance = name.and_then(NSAppearance::appearanceNamed);
    NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
}

/// The native window, for showing and hiding it without destroying it.
///
/// These calls make AppKit report focus changes back into GPUI, so they must
/// happen outside a GPUI window update or GPUI finds the window borrowed.
pub struct NativeWindow(Retained<NSWindow>);

impl NativeWindow {
    pub fn of(window: &Window) -> Option<Self> {
        ns_window(window).map(Self)
    }

    pub fn show(&self) {
        self.0.makeKeyAndOrderFront(None);
    }

    pub fn hide(&self) {
        self.0.orderOut(None);
    }

    /// Moves the window to the screen under the mouse pointer, centered
    /// horizontally with its top `top_fraction` of the way down.
    pub fn move_to_pointer_screen(&self, top_fraction: f64) {
        let pointer = NSEvent::mouseLocation();
        let screens = NSScreen::screens(self.0.mtm());
        let Some(screen) = screens.iter().find(|screen| {
            let f = screen.frame();
            pointer.x >= f.origin.x
                && pointer.x < f.origin.x + f.size.width
                && pointer.y >= f.origin.y
                && pointer.y < f.origin.y + f.size.height
        }) else {
            return;
        };
        let area = screen.frame();
        let size = self.0.frame().size;
        self.0.setFrameTopLeftPoint(NSPoint::new(
            area.origin.x + (area.size.width - size.width) / 2.,
            area.origin.y + area.size.height * (1. - top_fraction),
        ));
    }

    /// Finishes setting up a freshly created launcher window.
    pub fn configure_as_panel(&self) {
        // GPUI's pop-up is a titled panel; it has no use for window buttons.
        for button in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = self.0.standardWindowButton(button) {
                button.setHidden(true);
            }
        }
        // Appear and disappear instantly. AppKit's fade would otherwise show
        // the cleared, bar-sized content stretched over the full window.
        self.0.setAnimationBehavior(NSWindowAnimationBehavior::None);
    }


    /// Asks to make the app frontmost so a regular window (settings) can
    /// take focus. An accessory app can't reliably come forward, so it
    /// becomes a regular app (with a Dock icon) until [`run_in_background`].
    /// macOS may decline; check [`NativeWindow::app_is_active`].
    pub fn request_activation(&self) {
        let app = NSApplication::sharedApplication(self.0.mtm());
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }

    pub fn app_is_active(&self) -> bool {
        NSApplication::sharedApplication(self.0.mtm()).isActive()
    }

    /// Shows the window in front, focused only if the app is active.
    ///
    /// GPUI 0.2 deadlocks when a regular window becomes key while the app
    /// is inactive (its key-change handler calls `resignKeyWindow` while
    /// holding the window's lock), so an inactive app only orders it in
    /// front; the user's first click then focuses it.
    pub fn present(&self) {
        if self.app_is_active() {
            self.0.makeKeyAndOrderFront(None);
        } else {
            self.0.orderFrontRegardless();
        }
    }
}

/// Goes back to being an accessory app once no regular window is open.
pub fn run_in_background() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

/// Resizes the window while keeping its top edge in place. AppKit's
/// `setContentSize` keeps the bottom edge, which makes the bar jump upward.
pub fn set_content_height<T: 'static>(window: &mut Window, height: f32, cx: &mut Context<T>) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    // Resizing synchronously re-enters GPUI's resize callback while the window
    // is borrowed, so apply it on the next tick like GPUI's own `resize` does.
    cx.spawn(async move |_, _| {
        let frame = ns_window.frame();
        let content = ns_window.contentRectForFrameRect(frame);
        if content.size.height == height as f64 {
            return;
        }
        let mut new_frame = ns_window.frameRectForContentRect(NSRect::new(
            content.origin,
            NSSize::new(content.size.width, height as f64),
        ));
        new_frame.origin.y = frame.origin.y + frame.size.height - new_frame.size.height;
        ns_window.setFrame_display(new_frame, true);
    })
    .detach();
}

fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI hands out its live NSView, and we are on the main thread.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}
