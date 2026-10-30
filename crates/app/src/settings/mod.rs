//! The settings window: a sidebar of pages, each a few groups of settings.
//! Every change is saved immediately.

mod widgets;

use std::path::{Path, PathBuf};
use std::time::Duration;

use greatheights_ai::{Ask, AskEvent, Models};
use greatheights_core::{
    AGENT_PRESETS, AgentConfig, Appearance, CustomBang, CustomEngine, SearchEngine, Settings,
    Snippet, template_error,
};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, ElementId, Entity, FocusHandle, Focusable,
    FontWeight, Global, KeyBinding, KeyDownEvent, Keystroke, PathPromptOptions, SharedString,
    Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, actions, div, prelude::*, px, size,
};

use self::widgets::{
    Note, Tooltip, button, field_row, group, icon_button, input_box, note_row, pill, plain_row,
    radio_row, row, segment, segmented, switch,
};
use crate::input::{InputChanged, TextInput};
use crate::theme::{self, Theme};
use crate::window::{self, AskerHandle, Config};
use crate::{autostart, hotkey, platform};

actions!(settings, [Close, NextField]);

const CONTEXT: &str = "Settings";
const TEST_QUESTION: &str = "Reply with just the word OK.";
const SIDEBAR_WIDTH: f32 = 180.;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Close, Some(CONTEXT)),
        KeyBinding::new("secondary-w", Close, Some(CONTEXT)),
        KeyBinding::new("tab", NextField, Some(CONTEXT)),
    ]);
}

#[derive(Default)]
struct OpenWindow(Option<WindowHandle<SettingsView>>);

impl Global for OpenWindow {}

/// Opens the settings window, or brings it forward if it's already open.
pub fn open(cx: &mut App) {
    if let Some(handle) = cx.default_global::<OpenWindow>().0
        && handle.update(cx, |_, _, _| ()).is_ok()
    {
        bring_to_front(handle, cx);
        return;
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(780.), px(600.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Greatheights Settings".into()),
            ..Default::default()
        }),
        // Shown by `bring_to_front` once it's safe to focus; see
        // `NativeWindow::present`.
        focus: false,
        show: false,
        kind: WindowKind::Normal,
        is_resizable: true,
        is_minimizable: false,
        window_min_size: Some(size(px(620.), px(420.))),
        app_id: Some("greatheights".into()),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| cx.new(|cx| SettingsView::new(window, cx))) {
        Ok(handle) => {
            cx.set_global(OpenWindow(Some(handle)));
            bring_to_front(handle, cx);
        }
        Err(err) => log::error!("failed to open settings: {err}"),
    }
}

/// Debug aid: `GREATHEIGHTS_OPEN_SETTINGS` opens the window at startup (on
/// the page it names, if any), and `GREATHEIGHTS_TEST_AGENT` also presses
/// Test.
#[cfg(debug_assertions)]
pub fn debug_open(cx: &mut App) {
    let Some(page) = std::env::var_os("GREATHEIGHTS_OPEN_SETTINGS") else {
        return;
    };
    open(cx);
    let Some(handle) = cx.default_global::<OpenWindow>().0 else {
        return;
    };
    let page = Page::ALL
        .into_iter()
        .find(|p| p.title().eq_ignore_ascii_case(&page.to_string_lossy()));
    let _ = handle.update(cx, |view, window, cx| {
        if let Some(page) = page {
            view.page = page;
        }
        if std::env::var_os("GREATHEIGHTS_TEST_AGENT").is_some() {
            view.test(window, cx);
        }
    });
}

fn bring_to_front(handle: WindowHandle<SettingsView>, cx: &mut App) {
    let native = handle
        .update(cx, |_, window, _| platform::NativeWindow::of(window))
        .ok()
        .flatten();
    // Native window calls re-enter GPUI, so make them outside this update.
    cx.spawn(async move |cx| {
        let Some(native) = native else {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            return;
        };
        native.request_activation();
        // Activation is asynchronous; give it a moment.
        for _ in 0..15 {
            if native.app_is_active() {
                break;
            }
            cx.background_executor().timer(Duration::from_millis(20)).await;
        }
        native.present();
    })
    .detach();
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    General,
    Search,
    Apps,
    Snippets,
    QuickAsk,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::General,
        Page::Search,
        Page::Apps,
        Page::Snippets,
        Page::QuickAsk,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Search => "Search",
            Page::Apps => "Apps",
            Page::Snippets => "Snippets",
            Page::QuickAsk => "Quick Ask",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Page::General => "⚙",
            Page::Search => "⌕",
            Page::Apps => "▦",
            Page::Snippets => "¶",
            Page::QuickAsk => "?",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Page::General => "How the search bar opens and looks.",
            Page::Search => "What shows up as you type, and where web searches go.",
            Page::Apps => "Where apps are found, and which ones to leave out.",
            Page::Snippets => {
                "Type ; and a keyword in the search bar, then ↵ to copy the text. \
                 Built in: ;date ;time ;now ;uuid ;shrug ;lorem."
            }
            Page::QuickAsk => {
                "Press Tab in the search bar, or start with ?, to ask an AI agent. The agent \
                 signs in with its own account and can answer, but never edits files or \
                 runs commands."
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Choice {
    Off,
    Preset(usize),
    Custom,
}

enum TestState {
    Idle,
    Running {
        _ask: Ask,
        _task: Task<()>,
    },
    Passed(String),
    Failed(String),
}

/// The models the chosen agent offers. Listing them starts the agent.
enum ModelsState {
    /// Not asked for yet (a custom command still being typed).
    NotLoaded,
    Loading { _task: Task<()> },
    Loaded(Models),
    Failed(String),
}

/// An editable pair of fields in a list: a snippet's keyword and text, or a
/// bang's trigger and URL. Its subscriptions save edits and end with it.
struct PairRow {
    key: Entity<TextInput>,
    value: Entity<TextInput>,
    _subscriptions: [Subscription; 2],
}

pub struct SettingsView {
    focus: FocusHandle,
    page: Page,
    /// Everything not edited through a text field; the rest is read from the
    /// fields on save.
    settings: Settings,
    /// Read from the login item itself; see `autostart`.
    launch_at_login: bool,
    launch_at_login_error: Option<String>,
    /// Waiting for the user to press a new shortcut.
    recording: bool,
    hotkey_error: Option<String>,
    engine_name: Entity<TextInput>,
    engine_url: Entity<TextInput>,
    bangs: Vec<PairRow>,
    snippets: Vec<PairRow>,
    choice: Choice,
    /// The chosen model id; `None` is the agent's default.
    model: Option<String>,
    models: ModelsState,
    name: Entity<TextInput>,
    command: Entity<TextInput>,
    args: Entity<TextInput>,
    test: TestState,
    _subscriptions: Vec<Subscription>,
}

fn text_input(placeholder: &str, text: &str, cx: &mut App) -> Entity<TextInput> {
    cx.new(|cx| {
        let mut input = TextInput::new(placeholder.to_owned(), cx);
        input.set_text(text.to_owned(), cx);
        input
    })
}

impl SettingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.global::<Config>().settings.clone();
        let agent = settings.agent.clone();
        let choice = match &agent {
            None => Choice::Off,
            Some(agent) => AGENT_PRESETS
                .iter()
                .position(|preset| preset.config().same_launch(agent))
                .map_or(Choice::Custom, Choice::Preset),
        };
        // Custom fields start from the configured custom agent, if any.
        let custom = agent.clone().filter(|_| choice == Choice::Custom);
        let name = text_input(
            "My Agent",
            custom.as_ref().and_then(|a| a.name.as_deref()).unwrap_or_default(),
            cx,
        );
        let command = text_input(
            "my-agent",
            custom.as_ref().map(|a| a.command.as_str()).unwrap_or_default(),
            cx,
        );
        let args = text_input(
            "--acp",
            &custom.as_ref().map(|a| a.args.join(" ")).unwrap_or_default(),
            cx,
        );
        let engine_name = text_input("SearXNG", &settings.custom_engine.name, cx);
        let engine_url = text_input(
            "https://searx.example/search?q=%s",
            &settings.custom_engine.url,
            cx,
        );

        let mut subscriptions: Vec<Subscription> = [&name, &command, &args]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |this, _, _: &InputChanged, cx| {
                    if this.choice == Choice::Custom {
                        // A different command may offer different models.
                        this.model = None;
                        this.models = ModelsState::NotLoaded;
                        this.save(cx);
                    }
                })
            })
            .collect();
        for input in [&engine_name, &engine_url] {
            subscriptions.push(cx.subscribe(input, |this, _, _: &InputChanged, cx| this.save(cx)));
        }
        // Closing the window drops the view: hide the Dock icon again.
        subscriptions.push(cx.on_release(|_, cx| {
            cx.default_global::<OpenWindow>().0 = None;
            platform::run_in_background();
        }));

        let focus = cx.focus_handle();
        window.focus(&focus);
        let snippets = settings
            .snippets
            .iter()
            .map(|snippet| Self::snippet_row(snippet, cx))
            .collect();
        let bangs = settings
            .bangs
            .iter()
            .map(|bang| Self::bang_row(bang, cx))
            .collect();
        let mut view = Self {
            focus,
            page: Page::General,
            launch_at_login: autostart::is_enabled(),
            launch_at_login_error: None,
            recording: false,
            hotkey_error: None,
            engine_name,
            engine_url,
            bangs,
            snippets,
            choice,
            model: agent.and_then(|a| a.model),
            models: ModelsState::NotLoaded,
            name,
            command,
            args,
            test: TestState::Idle,
            settings,
            _subscriptions: subscriptions,
        };
        view.load_models(window, cx);
        view
    }

    fn agent(&self, cx: &App) -> Option<AgentConfig> {
        let mut agent = match self.choice {
            Choice::Off => return None,
            Choice::Preset(index) => AGENT_PRESETS[index].config(),
            Choice::Custom => {
                let command = self.command.read(cx).text().trim().to_owned();
                if command.is_empty() {
                    return None;
                }
                let name = self.name.read(cx).text().trim().to_owned();
                AgentConfig {
                    name: (!name.is_empty()).then_some(name),
                    command,
                    args: self
                        .args
                        .read(cx)
                        .text()
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                    env: Default::default(),
                    model: None,
                }
            }
        };
        agent.model = self.model.clone();
        Some(agent)
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let text = |input: &Entity<TextInput>| input.read(cx).text().to_owned();
        let mut settings = self.settings.clone();
        settings.agent = self.agent(cx);
        settings.custom_engine = CustomEngine {
            name: text(&self.engine_name).trim().to_owned(),
            url: text(&self.engine_url).trim().to_owned(),
        };
        settings.bangs = self
            .bangs
            .iter()
            .map(|row| CustomBang {
                trigger: text(&row.key).trim().trim_start_matches('!').to_owned(),
                url: text(&row.value).trim().to_owned(),
            })
            .filter(|bang| !bang.trigger.is_empty())
            .collect();
        settings.snippets = self
            .snippets
            .iter()
            .map(|row| Snippet {
                keyword: text(&row.key).trim().trim_start_matches(';').to_owned(),
                text: text(&row.value),
            })
            .filter(|snippet| !snippet.keyword.is_empty())
            .collect();
        self.settings = settings.clone();
        window::update_settings(|current| *current = settings, cx);
        // A previous result says nothing about the new agent.
        self.test = TestState::Idle;
        cx.notify();
    }

    /// Changes a setting that has no text field, and saves.
    fn change(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.settings);
        self.save(cx);
    }

    fn choose(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        if choice == self.choice {
            return;
        }
        self.choice = choice;
        self.model = None;
        self.models = ModelsState::NotLoaded;
        self.save(cx);
        match choice {
            Choice::Custom => window.focus(&self.command.focus_handle(cx)),
            _ => self.load_models(window, cx),
        }
    }

    fn choose_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        if model != self.model {
            self.model = model;
            self.save(cx);
        }
    }

    fn choose_engine(&mut self, engine: SearchEngine, window: &mut Window, cx: &mut Context<Self>) {
        if engine == self.settings.search_engine {
            return;
        }
        self.change(|s| s.search_engine = engine, cx);
        if engine == SearchEngine::Custom && self.engine_url.read(cx).text().is_empty() {
            window.focus(&self.engine_url.focus_handle(cx));
        }
    }

    fn load_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.agent(cx) else {
            self.models = ModelsState::NotLoaded;
            return;
        };
        let models = cx.global::<AskerHandle>().0.models(&agent);
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = models
                .recv()
                .await
                .unwrap_or_else(|_| Err("The agent stopped.".into()));
            let _ = this.update(cx, |this, cx| {
                this.models = match result {
                    Ok(models) => ModelsState::Loaded(models),
                    Err(err) => ModelsState::Failed(err),
                };
                cx.notify();
            });
        });
        self.models = ModelsState::Loading { _task: task };
        cx.notify();
    }

    pub fn test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.agent(cx) else {
            return;
        };
        let ask = cx.global::<AskerHandle>().0.ask(&agent, TEST_QUESTION);
        let events = ask.events.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let mut reply = String::new();
            let outcome = loop {
                match events.recv().await {
                    Ok(AskEvent::Text(text)) => reply.push_str(&text),
                    Ok(AskEvent::Done) => break TestState::Passed(reply.trim().to_owned()),
                    Ok(AskEvent::Failed(err)) => break TestState::Failed(err),
                    Err(_) => break TestState::Failed("The agent stopped.".into()),
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.test = outcome;
                cx.notify();
            });
        });
        self.test = TestState::Running {
            _ask: ask,
            _task: task,
        };
        cx.notify();
    }

    fn close(&mut self, _: &Close, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording {
            self.stop_recording(None, cx);
            return;
        }
        window.remove_window();
    }

    fn show_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording {
            self.stop_recording(None, cx);
        }
        self.page = page;
        // A field on the old page would otherwise keep taking keys.
        window.focus(&self.focus);
        cx.notify();
    }

    fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = true;
        self.hotkey_error = None;
        // Keys must reach this view, not a text field.
        window.focus(&self.focus);
        hotkey::pause(cx);
        cx.notify();
    }

    /// Ends recording, switching to `keystroke` if given and usable.
    fn stop_recording(&mut self, keystroke: Option<String>, cx: &mut Context<Self>) {
        self.recording = false;
        if let Some(keystroke) = keystroke {
            match hotkey::set(&keystroke, cx) {
                Ok(()) => {
                    self.change(|s| s.hotkey = keystroke, cx);
                    return;
                }
                Err(err) => self.hotkey_error = Some(err),
            }
        }
        // Keep the previous shortcut working.
        if let Err(err) = hotkey::set(&self.settings.hotkey, cx) {
            self.hotkey_error = Some(err);
        }
        cx.notify();
    }

    fn record_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        cx.stop_propagation();
        let pressed = Keystroke {
            modifiers: event.keystroke.modifiers,
            key: event.keystroke.key.clone(),
            key_char: None,
        };
        self.stop_recording(Some(pressed.unparse()), cx);
    }

    fn toggle_launch_at_login(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.launch_at_login;
        match autostart::set(enabled) {
            Ok(()) => {
                self.launch_at_login = enabled;
                self.launch_at_login_error = None;
            }
            Err(err) => self.launch_at_login_error = Some(err),
        }
        cx.notify();
    }

    /// Text fields on the current page, in Tab order.
    fn fields(&self) -> Vec<Entity<TextInput>> {
        match self.page {
            Page::General | Page::Apps => Vec::new(),
            Page::Search => {
                let mut fields = Vec::new();
                if self.settings.search_engine == SearchEngine::Custom {
                    fields.extend([self.engine_name.clone(), self.engine_url.clone()]);
                }
                for row in &self.bangs {
                    fields.extend([row.key.clone(), row.value.clone()]);
                }
                fields
            }
            Page::Snippets => self
                .snippets
                .iter()
                .flat_map(|row| [row.key.clone(), row.value.clone()])
                .collect(),
            Page::QuickAsk if self.choice == Choice::Custom => {
                vec![self.name.clone(), self.command.clone(), self.args.clone()]
            }
            Page::QuickAsk => Vec::new(),
        }
    }

    fn next_field(&mut self, _: &NextField, window: &mut Window, cx: &mut Context<Self>) {
        let fields = self.fields();
        if self.recording || fields.is_empty() {
            return;
        }
        let current = fields
            .iter()
            .position(|field| field.focus_handle(cx).is_focused(window));
        let next = current.map_or(0, |i| (i + 1) % fields.len());
        window.focus(&fields[next].focus_handle(cx));
    }

    fn pair_row(key: (&str, &str), value: (&str, &str), cx: &mut Context<Self>) -> PairRow {
        let key = text_input(key.0, key.1, cx);
        let value = text_input(value.0, value.1, cx);
        let save = |this: &mut Self, _: Entity<TextInput>, _: &InputChanged, cx: &mut Context<Self>| {
            this.save(cx)
        };
        PairRow {
            _subscriptions: [cx.subscribe(&key, save), cx.subscribe(&value, save)],
            key,
            value,
        }
    }

    fn bang_row(bang: &CustomBang, cx: &mut Context<Self>) -> PairRow {
        Self::pair_row(
            ("trigger", &bang.trigger),
            ("https://example.com/search?q=%s", &bang.url),
            cx,
        )
    }

    fn snippet_row(snippet: &Snippet, cx: &mut Context<Self>) -> PairRow {
        Self::pair_row(("keyword", &snippet.keyword), ("Text to copy", &snippet.text), cx)
    }

    fn add_row(&mut self, list: List, window: &mut Window, cx: &mut Context<Self>) {
        let row = match list {
            List::Bangs => Self::bang_row(&CustomBang::default(), cx),
            List::Snippets => Self::snippet_row(&Snippet::default(), cx),
        };
        window.focus(&row.key.focus_handle(cx));
        self.rows_mut(list).push(row);
        cx.notify();
    }

    fn remove_row(&mut self, list: List, index: usize, cx: &mut Context<Self>) {
        let rows = self.rows_mut(list);
        if index < rows.len() {
            rows.remove(index);
            self.save(cx);
        }
    }

    fn rows_mut(&mut self, list: List) -> &mut Vec<PairRow> {
        match list {
            List::Bangs => &mut self.bangs,
            List::Snippets => &mut self.snippets,
        }
    }

    /// Asks for app folders (`folders`) or apps to hide, and adds them.
    fn pick_paths(&mut self, folders: bool, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: Some(if folders { "Add".into() } else { "Hide".into() }),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.change(
                    |settings| {
                        let list = if folders {
                            &mut settings.app_folders
                        } else {
                            &mut settings.hidden_apps
                        };
                        for path in paths {
                            if !list.contains(&path) {
                                list.push(path);
                            }
                        }
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    fn render_sidebar(&self, cx: &Context<Self>) -> gpui::Div {
        let theme = cx.global::<Theme>();
        let items = Page::ALL.into_iter().map(|page| {
            let selected = page == self.page;
            div()
                .id(page.title())
                .h(px(30.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(4.))
                .text_size(px(13.))
                .map(|item| {
                    if selected {
                        item.bg(theme.row_selected)
                    } else {
                        item.text_color(theme.text_muted)
                            .hover(|item| item.bg(theme.row_hover).text_color(theme.text))
                    }
                })
                .child(
                    div()
                        .w(px(16.))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(page.glyph()),
                )
                .child(page.title())
                .on_click(cx.listener(move |this, _, window, cx| this.show_page(page, window, cx)))
        });
        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .p_2()
            .flex()
            .flex_col()
            .gap(px(2.))
            .bg(theme.background)
            .border_r_1()
            .border_color(theme.border_variant)
            .children(items)
    }

    fn render_general(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let theme = cx.global::<Theme>();
        let shortcut_note = match (&self.hotkey_error, self.recording) {
            (Some(err), _) => Note::Error(err.clone().into()),
            (None, true) => Note::Muted("Press the new shortcut, or Esc to cancel.".into()),
            (None, false) => Note::Muted("Opens and closes the search bar from anywhere.".into()),
        };
        let shortcut = button(
            "hotkey",
            if self.recording {
                "Press a shortcut…".to_owned()
            } else {
                hotkey::label(&self.settings.hotkey)
            },
            theme,
        )
        .min_w(px(110.))
        .justify_center()
        .font_family(theme::MONO_FONT)
        .when(self.recording, |b| b.border_color(theme.accent).bg(theme.selection))
        .on_click(cx.listener(|this, _, window, cx| {
            if !this.recording {
                this.start_recording(window, cx);
            }
        }));
        let shortcut_row = row("Shortcut", shortcut_note, shortcut, theme);

        let keep = self.settings.keep_last_query;
        let keep_row = row(
            "Remember last search",
            Note::Muted("Reopen with your last query, selected so typing replaces it.".into()),
            div()
                .id("keep-last-query")
                .child(switch(keep, theme))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change(|s| s.keep_last_query = !keep, cx)
                })),
            theme,
        );

        let appearances = [
            (Appearance::System, "System"),
            (Appearance::Light, "Light"),
            (Appearance::Dark, "Dark"),
        ]
        .into_iter()
        .map(|(appearance, label)| {
            segment(label, label, self.settings.appearance == appearance, theme).on_click(
                cx.listener(move |this, _, _, cx| {
                    if this.settings.appearance != appearance {
                        this.change(|s| s.appearance = appearance, cx)
                    }
                }),
            )
        });
        let theme_row = row(
            "Theme",
            Note::Muted("Zed's One Light and One Dark, or whichever the system uses.".into()),
            segmented(appearances, theme),
            theme,
        );

        let unavailable = autostart::executable().err();
        let login_note = match (&self.launch_at_login_error, &unavailable) {
            (Some(err), _) => Note::Error(err.clone().into()),
            (None, Some(reason)) => Note::Muted(reason.clone().into()),
            (None, None) => Note::Muted("Start in the background when you log in.".into()),
        };
        let login_row = row(
            "Launch at login",
            login_note,
            div()
                .id("launch-at-login")
                .when(unavailable.is_some(), |s| s.opacity(0.5))
                .child(switch(self.launch_at_login, theme))
                .when(unavailable.is_none(), |s| {
                    s.on_click(cx.listener(|this, _, _, cx| this.toggle_launch_at_login(cx)))
                }),
            theme,
        );

        vec![
            group(
                "Search bar",
                None,
                vec![shortcut_row.into_any_element(), keep_row.into_any_element()],
                theme,
            )
            .into_any_element(),
            group("Appearance", None, vec![theme_row.into_any_element()], theme).into_any_element(),
            group("Startup", None, vec![login_row.into_any_element()], theme).into_any_element(),
        ]
    }

    fn toggle_row(
        &self,
        id: &'static str,
        title: &'static str,
        description: &'static str,
        on: bool,
        set: fn(&mut Settings, bool),
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.global::<Theme>();
        row(
            title,
            Note::Muted(description.into()),
            div()
                .id(id)
                .child(switch(on, theme))
                .on_click(cx.listener(move |this, _, _, cx| this.change(|s| set(s, !on), cx))),
            theme,
        )
        .into_any_element()
    }

    fn render_search(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let settings = &self.settings;
        let results = vec![
            self.toggle_row(
                "calculator",
                "Calculator",
                "Math, units and currencies, like 12% of 80 or 20 usd in eur.",
                settings.calculator,
                |s, on| s.calculator = on,
                cx,
            ),
            self.toggle_row(
                "system-commands",
                "System commands",
                "Lock, sleep, restart, empty trash and dark mode.",
                settings.system_commands,
                |s, on| s.system_commands = on,
                cx,
            ),
            self.toggle_row(
                "web-fallback",
                "Web search",
                "A search for what you typed, below every other result. Bangs work either way.",
                settings.web_fallback,
                |s, on| s.web_fallback = on,
                cx,
            ),
        ];

        let theme = cx.global::<Theme>();
        let selected = self.settings.search_engine;
        let mut engines: Vec<AnyElement> = SearchEngine::ALL
            .into_iter()
            .chain([SearchEngine::Custom])
            .map(|engine| {
                let detail = engine
                    .template()
                    .map(|url| SharedString::from(display_host(url).to_owned()))
                    .or_else(|| Some("Any URL with %s".into()));
                radio_row(engine.name(), engine.name(), detail, engine == selected, theme)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.choose_engine(engine, window, cx)
                    }))
                    .into_any_element()
            })
            .collect();
        if selected == SearchEngine::Custom {
            let url = self.engine_url.read(cx).text().to_owned();
            let error = template_error(&url).filter(|_| !url.is_empty());
            engines.push(
                div()
                    .py_1()
                    .flex()
                    .flex_col()
                    .child(field_row("Name", &self.engine_name, false, theme, cx))
                    .child(field_row("URL", &self.engine_url, error.is_some(), theme, cx))
                    .child(
                        div()
                            .px_3()
                            .pb_2()
                            .pl(px(12. + 84. + 12.))
                            .text_size(px(12.))
                            .text_color(if error.is_some() { theme.error } else { theme.text_muted })
                            .child(match error {
                                Some(error) => error.to_owned(),
                                None if url.is_empty() => {
                                    "Until there's a URL, searches use Startpage.".to_owned()
                                }
                                None => format!("Searches go to {}.", display_host(&url)),
                            }),
                    )
                    .into_any_element(),
            );
        }

        let mut bangs: Vec<AnyElement> = self
            .bangs
            .iter()
            .enumerate()
            .map(|(index, bang)| {
                let url = bang.value.read(cx).text();
                let invalid = !url.is_empty() && template_error(url).is_some();
                self.render_pair(List::Bangs, index, "!", invalid, cx)
            })
            .collect();
        bangs.push(add_row_button(List::Bangs, "Add bang", cx));

        vec![
            group("Show in results", None, results, theme).into_any_element(),
            group(
                "Search engine",
                Some("Where a web search goes when no bang is used.".into()),
                engines,
                theme,
            )
            .into_any_element(),
            group(
                "Bangs",
                Some(
                    "Your own !shortcuts, used before the thousands built in. Put %s where \
                     the search terms go; without terms, the site itself opens."
                        .into(),
                ),
                bangs,
                theme,
            )
            .into_any_element(),
        ]
    }

    /// One editable entry of a list, with a button to remove it.
    fn render_pair(
        &self,
        list: List,
        index: usize,
        prefix: &'static str,
        invalid: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.global::<Theme>();
        let row = &self.rows(list)[index];
        plain_row()
            .child(
                div()
                    .w(px(150.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .w(px(10.))
                            .font_family(theme::MONO_FONT)
                            .text_color(theme.text_muted)
                            .child(prefix),
                    )
                    .child(input_box(&row.key, false, theme, cx).flex_1()),
            )
            .child(input_box(&row.value, invalid, theme, cx).flex_1())
            .child(
                icon_button(ElementId::Name(format!("{}-{index}", list.id()).into()), "×", theme)
                    .tooltip(|_, cx| cx.new(|_| Tooltip("Remove".into())).into())
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_row(list, index, cx))),
            )
            .into_any_element()
    }

    fn rows(&self, list: List) -> &Vec<PairRow> {
        match list {
            List::Bangs => &self.bangs,
            List::Snippets => &self.snippets,
        }
    }

    fn render_apps(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let theme = cx.global::<Theme>();
        let path_row = |id: String,
                        title: String,
                        detail: String,
                        action: &'static str,
                        remove: fn(&mut Settings) -> &mut Vec<PathBuf>,
                        index: usize| {
            let (title, detail) = (SharedString::from(title), SharedString::from(detail));
            plain_row()
                .min_h(px(44.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(13.)).truncate().child(title))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.text_muted)
                                .truncate()
                                .child(detail),
                        ),
                )
                .child(button(ElementId::Name(id.into()), action, theme).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.change(
                            |settings| {
                                let list = remove(settings);
                                if index < list.len() {
                                    list.remove(index);
                                }
                            },
                            cx,
                        )
                    },
                )))
        };

        let mut folders: Vec<AnyElement> = self
            .settings
            .app_folders
            .iter()
            .enumerate()
            .map(|(index, folder)| {
                path_row(
                    format!("folder-{index}"),
                    file_name(folder),
                    tilde(folder),
                    "Remove",
                    |s| &mut s.app_folders,
                    index,
                )
                .into_any_element()
            })
            .collect();
        if folders.is_empty() {
            folders.push(
                note_row("Apps in /Applications, ~/Applications and the system folders are always found.", theme)
                    .into_any_element(),
            );
        }
        folders.push(
            plain_row()
                .child(
                    button("add-folder", "Add Folder…", theme).on_click(
                        cx.listener(|this, _, window, cx| this.pick_paths(true, window, cx)),
                    ),
                )
                .into_any_element(),
        );

        let mut hidden: Vec<AnyElement> = self
            .settings
            .hidden_apps
            .iter()
            .enumerate()
            .map(|(index, app)| {
                path_row(
                    format!("hidden-{index}"),
                    app_name(app),
                    tilde(app),
                    "Show",
                    |s| &mut s.hidden_apps,
                    index,
                )
                .into_any_element()
            })
            .collect();
        if hidden.is_empty() {
            hidden.push(note_row("No apps are hidden.", theme).into_any_element());
        }
        hidden.push(
            plain_row()
                .child(
                    button("hide-app", "Hide an App…", theme).on_click(
                        cx.listener(|this, _, window, cx| this.pick_paths(false, window, cx)),
                    ),
                )
                .into_any_element(),
        );

        vec![
            group(
                "Extra folders",
                Some("Also searched for apps, along with the folders inside them.".into()),
                folders,
                theme,
            )
            .into_any_element(),
            group(
                "Hidden apps",
                Some("Left out of results, like helpers and uninstallers you never open.".into()),
                hidden,
                theme,
            )
            .into_any_element(),
        ]
    }

    fn render_snippets(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let theme = cx.global::<Theme>();
        let mut rows: Vec<AnyElement> = (0..self.snippets.len())
            .map(|index| self.render_pair(List::Snippets, index, ";", false, cx))
            .collect();
        if rows.is_empty() {
            rows.push(note_row("No snippets of your own yet.", theme).into_any_element());
        }
        rows.push(add_row_button(List::Snippets, "Add snippet", cx));
        vec![
            group(
                "Your snippets",
                Some("A snippet with a built-in's keyword replaces it.".into()),
                rows,
                theme,
            )
            .into_any_element(),
        ]
    }

    fn render_quick_ask(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let theme = cx.global::<Theme>();
        let option = |id: &'static str, choice: Choice, title: &'static str, detail: &'static str| {
            radio_row(id, title, Some(detail.into()), self.choice == choice, theme)
                .on_click(cx.listener(move |this, _, window, cx| this.choose(choice, window, cx)))
                .into_any_element()
        };
        let mut agents = vec![option("off", Choice::Off, "Off", "Quick Ask disabled")];
        for (index, preset) in AGENT_PRESETS.iter().enumerate() {
            agents.push(option(preset.name, Choice::Preset(index), preset.name, preset.sign_in));
        }
        agents.push(option("custom", Choice::Custom, "Custom command", "Any ACP agent"));
        if self.choice == Choice::Custom {
            agents.push(
                div()
                    .py_1()
                    .flex()
                    .flex_col()
                    .child(field_row("Name", &self.name, false, theme, cx))
                    .child(field_row("Command", &self.command, false, theme, cx))
                    .child(field_row("Arguments", &self.args, false, theme, cx))
                    .into_any_element(),
            );
        }

        let mut groups = vec![group("Agent", None, agents, theme).into_any_element()];
        if let Some(agent) = self.agent(cx) {
            let model = self.render_models(&agent, cx);
            let test = self.render_test(cx);
            let theme = cx.global::<Theme>();
            groups.push(group("Model", None, vec![model], theme).into_any_element());
            groups.push(test);
        }
        groups
    }

    fn render_models(&self, agent: &AgentConfig, cx: &Context<Self>) -> AnyElement {
        let theme = cx.global::<Theme>();
        let note = |text: String, color| {
            div()
                .text_size(px(12.))
                .text_color(color)
                .child(text)
                .into_any_element()
        };
        let load_button = |label: &'static str| {
            button("load-models", label, theme)
                .on_click(cx.listener(|this, _, window, cx| this.load_models(window, cx)))
                .into_any_element()
        };
        let content = match &self.models {
            ModelsState::NotLoaded => load_button("Load Models"),
            ModelsState::Loading { .. } => note(
                format!("Starting {} to list its models…", agent.display_name()),
                theme.text_muted,
            ),
            ModelsState::Failed(err) => div()
                .flex()
                .items_center()
                .gap_3()
                .child(load_button("Retry"))
                .child(note(err.clone(), theme.error))
                .into_any_element(),
            ModelsState::Loaded(models) if models.choices.is_empty() => note(
                format!("{} doesn't offer a model choice.", agent.display_name()),
                theme.text_muted,
            ),
            ModelsState::Loaded(models) => {
                let default_name = models
                    .default
                    .as_ref()
                    .and_then(|id| models.choices.iter().find(|c| &c.id == id))
                    .map_or("Default".to_owned(), |c| format!("Default ({})", c.name));
                let default = pill("model-default", default_name, self.model.is_none(), theme)
                    .on_click(cx.listener(|this, _, _, cx| this.choose_model(None, cx)));
                let choices = models.choices.iter().map(|choice| {
                    let id = choice.id.clone();
                    let selected = self.model.as_deref() == Some(choice.id.as_str());
                    pill(
                        SharedString::from(format!("model-{}", choice.id)),
                        choice.name.clone(),
                        selected,
                        theme,
                    )
                    .when_some(choice.description.clone(), |pill, description| {
                        pill.tooltip(move |_, cx| cx.new(|_| Tooltip(description.clone().into())).into())
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choose_model(Some(id.clone()), cx)
                    }))
                });
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(default)
                    .children(choices)
                    .into_any_element()
            }
        };
        plain_row().py_3().child(content).into_any_element()
    }

    fn render_test(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.global::<Theme>();
        let running = matches!(self.test, TestState::Running { .. });
        let (status, color) = match &self.test {
            TestState::Idle => (
                "Asks the agent to reply, to check it starts and is signed in.".to_owned(),
                theme.text_muted,
            ),
            TestState::Running { .. } => (
                "Starting the agent and asking it to reply…".into(),
                theme.text_muted,
            ),
            TestState::Passed(reply) => (format!("✓ Working. It replied “{reply}”."), theme.text),
            TestState::Failed(err) => (format!("✗ {err}"), theme.error),
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                button("test", "Test Agent", theme)
                    .when(running, |b| b.opacity(0.5))
                    .when(!running, |b| {
                        b.on_click(cx.listener(|this, _, window, cx| this.test(window, cx)))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(color)
                    .child(status),
            )
            .into_any_element()
    }
}

/// The two editable lists.
#[derive(Clone, Copy, PartialEq)]
enum List {
    Bangs,
    Snippets,
}

impl List {
    fn id(self) -> &'static str {
        match self {
            List::Bangs => "remove-bang",
            List::Snippets => "remove-snippet",
        }
    }
}

fn add_row_button(list: List, label: &'static str, cx: &Context<SettingsView>) -> AnyElement {
    let theme = cx.global::<Theme>();
    plain_row()
        .child(
            button(ElementId::Name(format!("add-{}", list.id()).into()), label, theme)
                .on_click(cx.listener(move |this, _, window, cx| this.add_row(list, window, cx))),
        )
        .into_any_element()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

/// "Chess" for `/Applications/Chess.app`.
fn app_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| file_name(path), |name| name.to_string_lossy().into_owned())
}

/// `https://www.google.com/search?q=%s` → `google.com`.
fn display_host(url: &str) -> &str {
    let host = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = host.split('/').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host)
}

/// The path with the home directory shortened to `~`.
fn tilde(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(PathBuf::from)) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;
        let groups = match page {
            Page::General => self.render_general(cx),
            Page::Search => self.render_search(cx),
            Page::Apps => self.render_apps(cx),
            Page::Snippets => self.render_snippets(cx),
            Page::QuickAsk => self.render_quick_ask(cx),
        };
        let sidebar = self.render_sidebar(cx);
        let theme = cx.global::<Theme>();

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::next_field))
            .on_key_down(cx.listener(Self::record_key))
            .size_full()
            .flex()
            .font_family(theme::UI_FONT)
            .bg(theme.field)
            .text_color(theme.text)
            .child(sidebar)
            .child(
                div()
                    // A fresh id per page starts it scrolled to the top.
                    .id(page.title())
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .max_w(px(640.))
                            .px_6()
                            .py_5()
                            .flex()
                            .flex_col()
                            .gap_6()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(page.title()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(theme.text_muted)
                                            .child(page.description()),
                                    ),
                            )
                            .children(groups),
                    ),
            )
    }
}
