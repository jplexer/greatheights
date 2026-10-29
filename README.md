<img src="assets/icon.svg" width="64" height="64" alt="">

# greatheights

Created in [T3 Code](https://t3.codes).

A minimal, keyboard-first launcher for macOS, Linux and Windows, built with
[GPUI](https://gpui.rs).

## Running

```sh
cargo run -p greatheights
```

Press <kbd>⌥ Space</kbd> to toggle the launcher. <kbd>↵</kbd> opens the
selected result, <kbd>⌘ ↵</kbd> runs its secondary action (show in Finder),
<kbd>Esc</kbd> closes it and <kbd>⌘ Q</kbd> quits.

Besides apps, the bar understands:

- **Math and units**: `12*7`, `5 km in mi` (↵ copies the result)
- **Bangs**: `!gh gpui` or `gpui !gh`, using [Kagi's bang list](https://github.com/kagisearch/bangs)
  (MIT, bundled). Anything else can be searched on Startpage.
- **System commands**: lock, sleep, restart, shut down, log out, empty
  trash, toggle dark mode. Restart, shut down and log out ask for a second ↵.

On Wayland, apps can't register global shortcuts; bind a compositor shortcut
instead (toggle command coming soon).

## Layout

- `crates/core`: result model, provider trait, fuzzy ranking, frecency
- `crates/providers`: result sources (apps, calculator, web/bangs, system commands)
- `crates/app`: the GPUI window, text input and hotkey
