//! The launcher window is created once and then hidden and shown, so the
//! hotkey doesn't pay for a new window and renderer each time. Hiding clears
//! the query, so every invocation starts empty.

use std::path::PathBuf;
use std::sync::Arc;

use greatheights_ai::Asker;
use greatheights_core::{Engine, Settings};
use greatheights_providers::{SnippetProvider, WebProvider};
use gpui::{
    App, AppContext, Bounds, Focusable, Global, ImgResourceLoader, Resource, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, point, px, size,
};

use crate::launcher::{self, Launcher};
use crate::platform;
use crate::theme::Theme;

const WIDTH: f32 = 640.;
/// Fraction of the screen height above the bar.
const TOP_OFFSET: f32 = 0.22;

pub struct EngineHandle(pub Arc<Engine>);

impl Global for EngineHandle {}

pub struct AskerHandle(pub Arc<Asker>);

impl Global for AskerHandle {}

/// The settings file and its contents, re-read every time the launcher opens
/// so edits apply without a restart.
pub struct Config {
    pub path: PathBuf,
    pub settings: Settings,
}

impl Global for Config {}

pub struct WebHandle(pub Arc<WebProvider>);

impl Global for WebHandle {}

pub struct SnippetsHandle(pub Arc<SnippetProvider>);

impl Global for SnippetsHandle {}

/// Installs new settings and passes them to the parts that cache them.
pub fn set_config(config: Config, cx: &mut App) {
    if let Some(web) = cx.try_global::<WebHandle>() {
        web.0.set_engine(config.settings.search_engine);
    }
    if let Some(snippets) = cx.try_global::<SnippetsHandle>() {
        snippets.0.set_snippets(config.settings.snippets.clone());
    }
    cx.set_global(config);
}

#[derive(Default)]
struct State {
    handle: Option<WindowHandle<Launcher>>,
    visible: bool,
}

impl Global for State {}

pub fn toggle(cx: &mut App) {
    if cx.default_global::<State>().visible {
        hide(cx);
    } else {
        show(cx);
    }
}

pub fn hide(cx: &mut App) {
    let state = cx.default_global::<State>();
    if !state.visible {
        return;
    }
    state.visible = false;
    let Some(handle) = state.handle else { return };

    let native = handle
        .update(cx, |_, window, _| platform::NativeWindow::of(window))
        .ok()
        .flatten();
    match native {
        // AppKit reports the focus change back into GPUI synchronously, so the
        // native call must run outside this update; see `NativeWindow`.
        Some(native) => cx
            .spawn(async move |cx| {
                native.hide();
                // Clear only once hidden, so the emptied bar is never on screen.
                let _ = handle.update(cx, |launcher, window, cx| launcher.reset(window, cx));
            })
            .detach(),
        None => {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            cx.default_global::<State>().handle = None;
        }
    }
}

pub fn show(cx: &mut App) {
    if cx.default_global::<State>().visible {
        return;
    }

    let engine = cx.global::<EngineHandle>().0.clone();
    refresh(engine.clone(), cx);
    cx.set_global(Theme::for_appearance(cx.window_appearance()));
    let path = cx.global::<Config>().path.clone();
    set_config(
        Config {
            settings: Settings::load(&path),
            path,
        },
        cx,
    );

    let (handle, fresh) = match cx.default_global::<State>().handle {
        Some(handle) => (handle, false),
        None => match open(engine, cx) {
            Some(handle) => (handle, true),
            None => return,
        },
    };
    let native = handle
        .update(cx, |_, window, _| platform::NativeWindow::of(window))
        .ok()
        .flatten();
    let state = cx.default_global::<State>();
    state.handle = Some(handle);
    state.visible = true;

    cx.spawn(async move |cx| {
        if let Some(native) = &native {
            // Restyling re-enters GPUI too, so it happens here, before the
            // window is first shown.
            if fresh {
                native.configure_as_panel();
            }
            // Open where the user is working, like Raycast.
            native.move_to_pointer_screen(TOP_OFFSET as f64);
            native.show();
        }
        let shown = handle.update(cx, |launcher, window, cx| {
            if native.is_none() {
                window.activate_window();
            }
            window.focus(&launcher.focus_handle(cx));
            #[cfg(debug_assertions)]
            if let Ok(query) = std::env::var("GREATHEIGHTS_QUERY") {
                launcher.set_query(query.clone(), cx);
                if std::env::var_os("GREATHEIGHTS_ASK").is_some() {
                    launcher.debug_ask(query, window, cx);
                }
            }
        });
        if let Err(err) = shown {
            log::error!("launcher window is gone: {err}");
            let _ = cx.update(|cx| *cx.default_global::<State>() = State::default());
        }
    })
    .detach();
}

/// Re-indexes in the background to pick up newly installed apps, then
/// preloads every icon so results never appear with icons missing.
fn refresh(engine: Arc<Engine>, cx: &mut App) {
    let indexed = cx.background_spawn({
        let engine = engine.clone();
        async move { engine.refresh() }
    });
    cx.spawn(async move |cx| {
        indexed.await;
        let Ok(loads) = cx.update(|cx| {
            engine
                .icons()
                .into_iter()
                .filter_map(|icon| {
                    let (load, is_new) =
                        cx.fetch_asset::<ImgResourceLoader>(&Resource::Path(icon.into()));
                    is_new.then_some(load)
                })
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        if loads.is_empty() {
            return;
        }
        for load in loads {
            let _ = load.await;
        }
        // An `img` that finds its load already started doesn't repaint when it
        // finishes, so repaint once everything has loaded.
        let _ = cx.update(|cx| {
            if let Some(handle) = cx.default_global::<State>().handle {
                let _ = handle.update(cx, |_, _, cx| cx.notify());
            }
        });
    })
    .detach();
}

fn open(engine: Arc<Engine>, cx: &mut App) -> Option<WindowHandle<Launcher>> {
    let Some(display) = cx.primary_display() else {
        log::error!("no display to show the launcher on");
        return None;
    };
    let area = display.bounds();
    let bounds = Bounds::new(
        point(
            area.origin.x + (area.size.width - px(WIDTH)) / 2.,
            area.origin.y + area.size.height * TOP_OFFSET,
        ),
        size(px(WIDTH), px(launcher::EMPTY_HEIGHT)),
    );

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        // On macOS the window is shown once it's been made a panel; see `show`.
        focus: !cfg!(target_os = "macos"),
        show: !cfg!(target_os = "macos"),
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: Some(display.id()),
        window_background: platform::WINDOW_BACKGROUND,
        app_id: Some("greatheights".into()),
        ..Default::default()
    };

    cx.open_window(options, |window, cx| {
        cx.new(|cx| Launcher::new(engine, window, cx))
    })
    .inspect_err(|err| log::error!("failed to open launcher window: {err}"))
    .ok()
}

/// Debug aid: with `GREATHEIGHTS_DEMO` set, cycles through queries and
/// hide/show so transitions can be watched without typing.
#[cfg(debug_assertions)]
pub fn run_demo(cx: &mut App) {
    if std::env::var_os("GREATHEIGHTS_DEMO").is_none() {
        return;
    }
    const STEPS: &[Option<&str>] = &[
        Some("t"),
        Some("te"),
        Some("term"),
        Some("zzzz"),
        Some(""),
        Some("sa"),
        None,
        None,
        Some("fi"),
    ];
    cx.spawn(async move |cx| {
        for step in STEPS.iter().cycle() {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(700))
                .await;
            log::debug!("demo step {step:?}");
            let result = cx.update(|cx| match step {
                Some(query) => {
                    if let Some(handle) = cx.default_global::<State>().handle {
                        let _ = handle.update(cx, |launcher, _, cx| {
                            launcher.set_query(query.to_string(), cx)
                        });
                    }
                }
                None => toggle(cx),
            });
            if result.is_err() {
                break;
            }
        }
    })
    .detach();
}
