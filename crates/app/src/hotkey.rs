//! System-wide shortcut that toggles the launcher.
//!
//! Shortcuts are stored in GPUI's keystroke format (`alt-space`,
//! `cmd-shift-k`) and converted for registration here. Wayland doesn't
//! allow apps to grab global shortcuts; there registration fails and users
//! bind a compositor shortcut instead.

use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::{App, Global, Keystroke};

use crate::window;

struct Registration {
    manager: GlobalHotKeyManager,
    active: Option<HotKey>,
}

impl Global for Registration {}

/// Sets up global shortcuts and registers `keystroke`.
pub fn init(keystroke: &str, cx: &mut App) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(err) => {
            log::warn!("global shortcuts unavailable: {err}");
            return;
        }
    };

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

    cx.set_global(Registration {
        manager,
        active: None,
    });
    if let Err(err) = set(keystroke, cx) {
        log::warn!("{err}");
    }
}

/// Switches the global shortcut to `keystroke`. On failure the previous
/// shortcut stays registered.
pub fn set(keystroke: &str, cx: &mut App) -> Result<(), String> {
    let hotkey = parse(keystroke)?;
    let Some(registration) = cx.try_global::<Registration>() else {
        return Err("Global shortcuts aren't available here.".into());
    };
    if registration.active == Some(hotkey) {
        return Ok(());
    }
    registration
        .manager
        .register(hotkey)
        .map_err(|_| format!("{} is already used by another app.", label(keystroke)))?;
    if let Some(old) = registration.active {
        let _ = registration.manager.unregister(old);
    }
    cx.global_mut::<Registration>().active = Some(hotkey);
    Ok(())
}

/// Turns the shortcut off while a new one is being recorded, so pressing
/// the current one doesn't toggle the launcher.
pub fn pause(cx: &mut App) {
    if let Some(registration) = cx.try_global::<Registration>()
        && let Some(active) = registration.active
    {
        let _ = registration.manager.unregister(active);
        cx.global_mut::<Registration>().active = None;
    }
}

/// Converts a GPUI keystroke into a global shortcut. It needs a modifier
/// (other than Shift alone) unless it's a function key, so it can't steal
/// plain typing.
pub fn parse(keystroke: &str) -> Result<HotKey, String> {
    let keystroke =
        Keystroke::parse(keystroke).map_err(|_| format!("“{keystroke}” isn't a shortcut."))?;
    let code = code(&keystroke.key)
        .ok_or_else(|| format!("{} can't be used in a shortcut.", key_label(&keystroke.key)))?;
    let m = &keystroke.modifiers;
    let is_function_key = keystroke.key.starts_with('f') && keystroke.key.len() > 1;
    if !(m.control || m.alt || m.platform || is_function_key) {
        return Err("Add ⌘, ⌥ or ⌃ so the shortcut doesn't steal typing.".into());
    }

    let mut modifiers = Modifiers::empty();
    modifiers.set(Modifiers::CONTROL, m.control);
    modifiers.set(Modifiers::ALT, m.alt);
    modifiers.set(Modifiers::SHIFT, m.shift);
    modifiers.set(Modifiers::META, m.platform);
    Ok(HotKey::new(Some(modifiers), code))
}

fn code(key: &str) -> Option<Code> {
    let name = match key {
        "space" => "Space",
        "enter" => "Enter",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "-" => "Minus",
        "=" => "Equal",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "\\" => "Backslash",
        ";" => "Semicolon",
        "'" => "Quote",
        "," => "Comma",
        "." => "Period",
        "/" => "Slash",
        "`" => "Backquote",
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase() => {
            return Code::from_str(&format!("Key{}", key.to_ascii_uppercase())).ok();
        }
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() => {
            return Code::from_str(&format!("Digit{key}")).ok();
        }
        key if key.starts_with('f') && key[1..].parse::<u8>().is_ok_and(|n| (1..=20).contains(&n)) => {
            return Code::from_str(&key.to_ascii_uppercase()).ok();
        }
        _ => return None,
    };
    Code::from_str(name).ok()
}

/// How a shortcut is shown, e.g. "⌥ Space" on macOS or "Alt+Space" elsewhere.
pub fn label(keystroke: &str) -> String {
    let Ok(keystroke) = Keystroke::parse(keystroke) else {
        return keystroke.to_owned();
    };
    let m = &keystroke.modifiers;
    let key = key_label(&keystroke.key);
    if cfg!(target_os = "macos") {
        let mut label = String::new();
        for (on, symbol) in [(m.control, "⌃"), (m.alt, "⌥"), (m.shift, "⇧"), (m.platform, "⌘")] {
            if on {
                label.push_str(symbol);
            }
        }
        format!("{label} {key}")
    } else {
        let mut parts = Vec::new();
        for (on, name) in [(m.control, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift"), (m.platform, "Super")] {
            if on {
                parts.push(name.to_owned());
            }
        }
        parts.push(key);
        parts.join("+")
    }
}

fn key_label(key: &str) -> String {
    match key {
        "space" => "Space".into(),
        "enter" => "Return".into(),
        "backspace" => "Delete".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        key if key.chars().count() == 1 => key.to_uppercase(),
        key => {
            let mut chars = key.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_shortcuts() {
        assert_eq!(
            parse("alt-space").unwrap(),
            HotKey::new(Some(Modifiers::ALT), Code::Space)
        );
        assert_eq!(
            parse("cmd-shift-k").unwrap(),
            HotKey::new(Some(Modifiers::META | Modifiers::SHIFT), Code::KeyK)
        );
        assert_eq!(parse("f5").unwrap(), HotKey::new(None, Code::F5));
    }

    #[test]
    fn rejects_shortcuts_that_would_steal_typing() {
        assert!(parse("k").is_err());
        assert!(parse("shift-k").is_err());
        assert!(parse("cmd-escape").is_err());
    }

    #[test]
    fn labels_shortcuts() {
        if cfg!(target_os = "macos") {
            assert_eq!(label("alt-space"), "⌥ Space");
            assert_eq!(label("cmd-shift-k"), "⇧⌘ K");
        } else {
            assert_eq!(label("alt-space"), "Alt+Space");
        }
    }
}
