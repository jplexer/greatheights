//! System-wide shortcut that toggles the launcher.
//!
//! Wayland doesn't allow apps to grab global shortcuts; there the
//! registration fails and users bind a compositor shortcut instead.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::{App, Global};

use crate::window;

/// Keeps the registration alive for the life of the app.
struct Registration(#[allow(dead_code)] GlobalHotKeyManager);

impl Global for Registration {}

pub fn register(cx: &mut App) {
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(err) => {
            log::warn!("global shortcuts unavailable: {err}");
            return;
        }
    };
    if let Err(err) = manager.register(hotkey) {
        log::warn!("couldn't register {hotkey}: {err}");
        return;
    }

    let (tx, rx) = async_channel::unbounded();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed {
            let _ = tx.try_send(());
        }
    }));
    cx.spawn(async move |cx| {
        while rx.recv().await.is_ok() {
            if cx.update(window::toggle).is_err() {
                break;
            }
        }
    })
    .detach();

    cx.set_global(Registration(manager));
}
