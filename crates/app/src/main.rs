mod answer;
mod hotkey;
mod input;
mod launcher;
mod platform;
mod settings;
mod theme;
mod window;

use std::path::PathBuf;
use std::sync::Arc;

use greatheights_ai::Asker;
use greatheights_core::{Engine, Frecency, Settings, settings_path};
use greatheights_providers::{AppProvider, CalcProvider, SystemProvider, WebProvider};
use gpui::Application;

use crate::theme::Theme;
use crate::window::{AskerHandle, Config, EngineHandle};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let config_path = settings_path(&app_dir(dirs::config_dir()));
    // Debug aid: try other settings without touching the real file.
    #[cfg(debug_assertions)]
    let config_path = std::env::var_os("GREATHEIGHTS_SETTINGS")
        .map(std::path::PathBuf::from)
        .unwrap_or(config_path);
    let engine = Arc::new(build_engine());

    Application::new().run(move |cx| {
        platform::init();
        Theme::init(cx);
        input::bind_keys(cx);
        launcher::bind_keys(cx);
        settings::bind_keys(cx);
        cx.set_global(EngineHandle(engine));
        cx.set_global(AskerHandle(Arc::new(Asker::new())));
        cx.set_global(Config {
            settings: Settings::load(&config_path),
            path: config_path,
        });
        hotkey::register(cx);
        window::show(cx);
        #[cfg(debug_assertions)]
        window::run_demo(cx);
        #[cfg(debug_assertions)]
        settings::debug_open(cx);
    });
}

fn build_engine() -> Engine {
    let cache = app_dir(dirs::cache_dir());
    let data = app_dir(dirs::data_dir());
    Engine::new(
        vec![
            Arc::new(AppProvider::new(cache.join("icons"))),
            Arc::new(SystemProvider::new()),
            Arc::new(CalcProvider::new(cache.join("rates.json"))),
            Arc::new(WebProvider::new()),
        ],
        Frecency::load(data.join("frecency.json")),
    )
}

fn app_dir(base: Option<PathBuf>) -> PathBuf {
    base.unwrap_or_else(std::env::temp_dir).join("greatheights")
}
