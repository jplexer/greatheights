mod hotkey;
mod input;
mod launcher;
mod platform;
mod theme;
mod window;

use std::path::PathBuf;
use std::sync::Arc;

use greatheights_core::{Engine, Frecency};
use greatheights_providers::{AppProvider, CalcProvider, SystemProvider, WebProvider};
use gpui::Application;

use crate::theme::Theme;
use crate::window::EngineHandle;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let engine = Arc::new(build_engine());

    Application::new().run(move |cx| {
        platform::init();
        Theme::init(cx);
        input::bind_keys(cx);
        launcher::bind_keys(cx);
        cx.set_global(EngineHandle(engine));
        hotkey::register(cx);
        window::show(cx);
        #[cfg(debug_assertions)]
        window::run_demo(cx);
    });
}

fn build_engine() -> Engine {
    let cache = app_dir(dirs::cache_dir());
    let data = app_dir(dirs::data_dir());
    Engine::new(
        vec![
            Arc::new(AppProvider::new(cache.join("icons"))),
            Arc::new(SystemProvider::new()),
            Arc::new(CalcProvider::new()),
            Arc::new(WebProvider::new()),
        ],
        Frecency::load(data.join("frecency.json")),
    )
}

fn app_dir(base: Option<PathBuf>) -> PathBuf {
    base.unwrap_or_else(std::env::temp_dir).join("greatheights")
}
