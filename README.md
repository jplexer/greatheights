<img src="assets/icon.svg" width="64" height="64" alt="">

# greatheights

Created in [T3 Code](https://t3.codes).

A minimal, keyboard-first launcher for macOS, Linux and Windows, built with
[GPUI](https://gpui.rs).

## Installing (macOS)

```sh
script/bundle-macos.sh --install
```

This builds `Greatheights.app` (ad-hoc signed, for this Mac only), copies it
to `/Applications` and starts it. Turn on **Launch at login** in Settings to
have it start quietly in the background when you log in. Opening the app
while it's running shows the search bar.

## Running

```sh
cargo run -p greatheights
```

Press <kbd>⌥ Space</kbd> (changeable in Settings) to toggle the launcher; it
opens on the screen with the mouse pointer. <kbd>↵</kbd> opens the
selected result, <kbd>⌘ ↵</kbd> runs its secondary action (show in Finder),
<kbd>Esc</kbd> closes it and <kbd>⌘ Q</kbd> quits.

Besides apps, the bar understands:

- **Math, units and currency**: `12*7`, `5 km in mi`, `100 usd to eur`
  (↵ copies the result). Exchange rates are the European Central Bank's
  daily reference rates (about 30 currencies), cached for offline use.
- **Bangs**: `!gh gpui` or `gpui !gh`, using [Kagi's bang list](https://github.com/kagisearch/bangs)
  (MIT, bundled), plus your own bangs from Settings, which take priority.
  Anything else can be searched with your chosen engine: Startpage
  (default), Kagi, Google, DuckDuckGo, Bing, Ecosia or any URL with `%s`.
- **Snippets**: type `;` and a keyword, then ↵ copies the text. Built in:
  `;date`, `;time`, `;now`, `;uuid`, `;shrug`, `;lorem`; add your own in
  Settings.
- **System commands**: lock, sleep, restart, shut down, log out, empty
  trash, toggle dark mode. Restart, shut down and log out ask for a second ↵.

The calculator, system commands and web search can each be turned off in
Settings, which also has extra app folders, hidden apps, a light/dark theme
override and an option to reopen with the last query.

## Quick Ask

Press <kbd>Tab</kbd> to ask an AI agent about whatever you typed, or start
the query with `?`. Answers stream in below the bar with Markdown formatting;
<kbd>↵</kbd> copies the Markdown. To continue the chat, press <kbd>Tab</kbd>
under a finished answer and type a follow-up: it goes to the same agent
session, so the agent remembers the conversation. The chat ends when the
launcher closes.

Any [Agent Client Protocol](https://agentclientprotocol.com) agent works, so
you can use the Claude, Codex or Gemini subscription you already have. There
is no default: pick one in the settings window (search "Greatheights
Settings", or press Tab before choosing one). It offers Claude, Codex,
Gemini or any custom ACP command, lists the models the agent offers so you
can pick one, and has a Test button to check it works.

The agent signs in with its own CLI (for Claude, the Claude Code login).
Each question runs in an empty scratch folder and every permission request
is declined, so agents can answer but never edit files or run commands.

On Wayland, apps can't register global shortcuts; bind a compositor shortcut
instead (toggle command coming soon).

## Layout

- `crates/core`: result model, provider trait, fuzzy ranking, frecency
- `crates/providers`: result sources (apps, calculator, web/bangs, snippets, system commands)
- `crates/ai`: Quick Ask over ACP, with a scripted mock agent for tests
- `crates/app`: the GPUI window, text input and hotkey

## Credits

The UI follows Zed's default One Dark and One Light themes and bundles the
same fonts Zed does: [IBM Plex Sans](https://github.com/IBM/plex) and
[Lilex](https://github.com/mishamyrt/Lilex), both under the SIL Open Font
License (see `crates/app/assets/fonts`).
