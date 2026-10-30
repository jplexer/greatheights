use gpui::{Context, Window, WindowBackgroundAppearance};
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSGlassEffectView,
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindow, NSWindowAnimationBehavior, NSWindowButton, NSWindowOrderingMode,
};
use objc2_foundation::{NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub const UI_FONT: &str = ".SystemUIFont";

/// We draw our own backdrop view; GPUI's `Blurred` strips the blur on recent macOS.
pub const WINDOW_BACKGROUND: WindowBackgroundAppearance = WindowBackgroundAppearance::Transparent;

/// The native backdrop already separates the window from what's behind it.
pub const HAS_BACKDROP: bool = true;

const CORNER_RADIUS: f64 = 18.;

/// Runs as an accessory: no Dock icon, no menu bar.
pub fn init() {
    let mtm = MainThreadMarker::new().expect("init must run on the main thread");
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

pub fn configure_window(window: &Window) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    for button in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = ns_window.standardWindowButton(button) {
            button.setHidden(true);
        }
    }

    // Appear and disappear instantly. AppKit's fade would otherwise show
    // the cleared, bar-sized content stretched over the full window.
    ns_window.setAnimationBehavior(NSWindowAnimationBehavior::None);

    let Some(content) = ns_window.contentView() else {
        return;
    };
    let backdrop = backdrop_view(content.bounds(), ns_window.mtm());
    backdrop.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    content.addSubview_positioned_relativeTo(&backdrop, NSWindowOrderingMode::Below, None);
}

/// Liquid Glass where available (macOS 26+), a vibrancy blur otherwise.
fn backdrop_view(frame: NSRect, mtm: MainThreadMarker) -> Retained<NSView> {
    if AnyClass::get(c"NSGlassEffectView").is_some() {
        let glass = NSGlassEffectView::initWithFrame(NSGlassEffectView::alloc(mtm), frame);
        glass.setCornerRadius(CORNER_RADIUS);
        return Retained::into_super(glass);
    }
    let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
    effect.setMaterial(NSVisualEffectMaterial::HUDWindow);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
    Retained::into_super(effect)
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
