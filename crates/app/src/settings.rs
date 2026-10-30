//! The settings window. Every change is saved immediately.

use std::time::Duration;

use greatheights_ai::{Ask, AskEvent, Models};
use greatheights_core::{AGENT_PRESETS, AgentConfig, SearchEngine, Settings, Snippet};
use gpui::{
    AnyElement, App, AppContext, KeyDownEvent, Keystroke, Bounds, Context, Div, ElementId, Entity, FocusHandle, Focusable,
    Global, KeyBinding, MouseButton, SharedString, Stateful, Subscription, Task, TitlebarOptions,
    Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, actions, div, prelude::*, px,
    size,
};

use crate::input::{InputChanged, TextInput};
use crate::{autostart, hotkey, platform};
use crate::theme::{self, Theme};
use crate::window::{self, AskerHandle, Config};

actions!(settings, [Close, NextField]);

const CONTEXT: &str = "Settings";
const TEST_QUESTION: &str = "Reply with just the word OK.";

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
            size(px(540.), px(800.)),
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

/// Debug aid: `GREATHEIGHTS_OPEN_SETTINGS` opens the window at startup, and
/// `GREATHEIGHTS_TEST_AGENT` also presses Test.
#[cfg(debug_assertions)]
pub fn debug_open(cx: &mut App) {
    if std::env::var_os("GREATHEIGHTS_OPEN_SETTINGS").is_none() {
        return;
    }
    open(cx);
    if std::env::var_os("GREATHEIGHTS_TEST_AGENT").is_some()
        && let Some(handle) = cx.default_global::<OpenWindow>().0
    {
        let _ = handle.update(cx, |view, window, cx| view.test(window, cx));
    }
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

/// One editable snippet. Its subscriptions save edits and end with it.
struct SnippetRow {
    keyword: Entity<TextInput>,
    text: Entity<TextInput>,
    _subscriptions: [Subscription; 2],
}

pub struct SettingsView {
    focus: FocusHandle,
    /// Read from the login item itself; see `autostart`.
    launch_at_login: bool,
    launch_at_login_error: Option<String>,
    snippets: Vec<SnippetRow>,
    hotkey: String,
    /// Waiting for the user to press a new shortcut.
    recording: bool,
    hotkey_error: Option<String>,
    search_engine: SearchEngine,
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
        let field = |placeholder: &str, text: String, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(placeholder.to_owned(), cx);
                input.set_text(text, cx);
                input
            })
        };
        let name = field(
            "My Agent",
            custom.as_ref().and_then(|a| a.name.clone()).unwrap_or_default(),
            cx,
        );
        let command = field(
            "my-agent",
            custom.as_ref().map(|a| a.command.clone()).unwrap_or_default(),
            cx,
        );
        let args = field(
            "--acp",
            custom.as_ref().map(|a| a.args.join(" ")).unwrap_or_default(),
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
        let mut view = Self {
            focus,
            launch_at_login: autostart::is_enabled(),
            launch_at_login_error: None,
            snippets,
            hotkey: settings.hotkey.clone(),
            recording: false,
            hotkey_error: None,
            search_engine: settings.search_engine,
            choice,
            model: agent.and_then(|a| a.model),
            models: ModelsState::NotLoaded,
            name,
            command,
            args,
            test: TestState::Idle,
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
        let settings = Settings {
            hotkey: self.hotkey.clone(),
            agent: self.agent(cx),
            search_engine: self.search_engine,
            snippets: self
                .snippets
                .iter()
                .map(|row| Snippet {
                    keyword: row
                        .keyword
                        .read(cx)
                        .text()
                        .trim()
                        .trim_start_matches(';')
                        .to_owned(),
                    text: row.text.read(cx).text().to_owned(),
                })
                .filter(|snippet| !snippet.keyword.is_empty())
                .collect(),
        };
        let path = cx.global::<Config>().path.clone();
        if let Err(err) = settings.save(&path) {
            log::error!("couldn't save settings to {}: {err}", path.display());
        }
        window::set_config(Config { path, settings }, cx);
        // A previous result says nothing about the new agent.
        self.test = TestState::Idle;
        cx.notify();
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

    fn choose_engine(&mut self, engine: SearchEngine, cx: &mut Context<Self>) {
        if engine != self.search_engine {
            self.search_engine = engine;
            self.save(cx);
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
                    self.hotkey = keystroke;
                    self.save(cx);
                    return;
                }
                Err(err) => self.hotkey_error = Some(err),
            }
        }
        // Keep the previous shortcut working.
        if let Err(err) = hotkey::set(&self.hotkey, cx) {
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

    fn render_launch_at_login(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let unavailable = autostart::executable().err();
        let note = self.launch_at_login_error.clone().or(unavailable.clone());
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(label_cell("Startup", theme))
            .child(
                div()
                    .id("launch-at-login")
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(13.))
                    .child(checkbox(self.launch_at_login, theme))
                    .child("Launch at login")
                    .when(unavailable.is_some(), |row| row.text_color(theme.text_muted))
                    .when(unavailable.is_none(), |row| {
                        row.on_click(cx.listener(|this, _, _, cx| this.toggle_launch_at_login(cx)))
                    }),
            )
            .children(note.map(|note| {
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(if self.launch_at_login_error.is_some() {
                        theme.error
                    } else {
                        theme.text_muted
                    })
                    .child(note)
            }))
    }

    fn render_hotkey(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let label = if self.recording {
            "Press a shortcut…".to_owned()
        } else {
            hotkey::label(&self.hotkey)
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(label_cell("Shortcut", theme))
            .child(
                pill("hotkey", label, self.recording, theme)
                    .font_family(theme::MONO_FONT)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if !this.recording {
                            this.start_recording(window, cx);
                        }
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .map(|note| match (&self.hotkey_error, self.recording) {
                        (Some(err), _) => note.text_color(theme.error).child(err.clone()),
                        (None, true) => note
                            .text_color(theme.text_muted)
                            .child("Esc to cancel"),
                        (None, false) => note,
                    }),
            )
    }

    fn next_field(&mut self, _: &NextField, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording || self.choice != Choice::Custom {
            return;
        }
        let fields = [&self.name, &self.command, &self.args];
        let current = fields
            .iter()
            .position(|field| field.focus_handle(cx).is_focused(window));
        let next = current.map_or(0, |i| (i + 1) % fields.len());
        window.focus(&fields[next].focus_handle(cx));
    }

    fn render_option(
        &self,
        id: &'static str,
        choice: Choice,
        title: SharedString,
        detail: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let selected = self.choice == choice;
        div()
            .id(id)
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(4.))
            .when(selected, |row| row.bg(theme.row_selected))
            .when(!selected, |row| row.hover(|row| row.bg(theme.row_hover)))
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if selected { theme.accent } else { theme.text_muted })
                    .when(selected, |ring| {
                        ring.child(div().size(px(8.)).rounded_full().bg(theme.accent))
                    }),
            )
            .child(div().flex_1().text_size(px(14.)).child(title))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .child(detail),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.choose(choice, window, cx)))
    }

    fn render_field(
        &self,
        label: &'static str,
        input: &Entity<TextInput>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(label_cell(label, theme))
            .child(input_box(input, theme, cx).flex_1())
    }

    fn snippet_row(snippet: &Snippet, cx: &mut Context<Self>) -> SnippetRow {
        let input = |placeholder: &str, text: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(placeholder.to_owned(), cx);
                input.set_text(text.to_owned(), cx);
                input
            })
        };
        let keyword = input("keyword", &snippet.keyword, cx);
        let text = input("Text to copy", &snippet.text, cx);
        let save = |this: &mut Self, _: Entity<TextInput>, _: &InputChanged, cx: &mut Context<Self>| {
            this.save(cx)
        };
        SnippetRow {
            _subscriptions: [cx.subscribe(&keyword, save), cx.subscribe(&text, save)],
            keyword,
            text,
        }
    }

    fn add_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = Self::snippet_row(&Snippet::default(), cx);
        window.focus(&row.keyword.focus_handle(cx));
        self.snippets.push(row);
        cx.notify();
    }

    fn remove_snippet(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.snippets.len() {
            self.snippets.remove(index);
            self.save(cx);
        }
    }

    fn render_snippets(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let rows = self.snippets.iter().enumerate().map(|(index, row)| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(130.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .font_family(theme::MONO_FONT)
                                .text_color(theme.text_muted)
                                .child(";"),
                        )
                        .child(input_box(&row.keyword, theme, cx).flex_1()),
                )
                .child(input_box(&row.text, theme, cx).flex_1())
                .child(
                    pill(SharedString::from(format!("remove-{index}")), "Remove", false, theme)
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_snippet(index, cx))),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(rows)
            .child(
                div().flex().child(
                    pill("add-snippet", "Add snippet", false, theme)
                        .on_click(cx.listener(|this, _, window, cx| this.add_snippet(window, cx))),
                ),
            )
    }

    fn render_engines(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let pills = SearchEngine::ALL.into_iter().map(|engine| {
            pill(engine.name(), engine.name(), engine == self.search_engine, theme)
                .on_click(cx.listener(move |this, _, _, cx| this.choose_engine(engine, cx)))
        });
        div().flex().flex_wrap().gap_2().children(pills)
    }

    fn render_models(&self, agent: &AgentConfig, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<Theme>();
        let note = |text: String, color| {
            div()
                .text_size(px(12.))
                .text_color(color)
                .child(text)
                .into_any_element()
        };
        let load_button = |label: &'static str| {
            pill("load-models", label, false, theme)
                .on_click(cx.listener(|this, _, window, cx| this.load_models(window, cx)))
                .into_any_element()
        };
        let content = match &self.models {
            ModelsState::NotLoaded => load_button("Load models"),
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
        div()
            .flex()
            .items_start()
            .gap_3()
            .child(label_cell("Model", theme).pt(px(4.)))
            .child(div().flex_1().min_w_0().child(content))
            .into_any_element()
    }

    fn render_test(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let can_test = self.agent(cx).is_some() && !matches!(self.test, TestState::Running { .. });
        let (status, color) = match &self.test {
            TestState::Idle => (String::new(), theme.text_muted),
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
                div()
                    .id("test")
                    .flex_none()
                    .px_3()
                    .py_1()
                    .rounded(px(4.))
                    .bg(theme.field)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(13.))
                    .when(!can_test, |button| button.text_color(theme.text_muted))
                    .when(can_test, |button| {
                        button
                            .hover(|button| button.bg(theme.row_hover))
                            .on_click(cx.listener(|this, _, window, cx| this.test(window, cx)))
                    })
                    .child("Test"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(color)
                    .child(status),
            )
    }
}

fn section(title: &'static str, description: &'static str, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_size(px(17.)).child(title))
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.text_muted)
                .child(description),
        )
}

fn checkbox(checked: bool, theme: &Theme) -> Div {
    div()
        .size(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .text_size(px(11.))
        .map(|box_| {
            if checked {
                box_.bg(theme.accent).border_color(theme.accent).text_color(theme.background).child("✓")
            } else {
                box_.bg(theme.field).border_color(theme.border)
            }
        })
}

/// A bordered box around a text input, like Zed's input fields.
fn input_box(input: &Entity<TextInput>, theme: &Theme, cx: &App) -> Div {
    let focus = input.focus_handle(cx);
    div()
        .min_w_0()
        .overflow_hidden()
        .h(px(30.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(theme.field)
        .border_1()
        .border_color(theme.border)
        .font_family(theme::MONO_FONT)
        .text_size(px(13.))
        .line_height(px(18.))
        .on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
        .child(input.clone())
}

fn label_cell(label: &'static str, theme: &Theme) -> Div {
    div()
        .w(px(84.))
        .flex_none()
        .text_size(px(13.))
        .text_color(theme.text_muted)
        .child(label)
}

/// A selectable rounded chip, for short lists of choices.
fn pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded(px(4.))
        .border_1()
        .text_size(px(13.))
        .map(|pill| {
            if selected {
                pill.bg(theme.selection).border_color(theme.accent)
            } else {
                pill.bg(theme.field)
                    .border_color(theme.border)
                    .hover(|pill| pill.bg(theme.row_hover))
            }
        })
        .child(label.into())
}

/// A plain text tooltip.
struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .text_size(px(12.))
            .text_color(theme.text)
            .font_family(theme::UI_FONT)
            .child(self.0.clone())
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let (text, surface, border) = (theme.text, theme.background, theme.border_variant);
        let general = section(
            "General",
            "How the search bar opens.",
            theme,
        );
        let search = section(
            "Search",
            "Where a query goes when nothing else matches. Bangs like !g always work.",
            theme,
        );
        let snippets = section(
            "Snippets",
            "Type ; and a keyword in the search bar, then ↵ to copy the text. Built in: \
             ;date ;time ;now ;uuid ;shrug ;lorem.",
            theme,
        );
        let quick_ask = section(
            "Quick Ask",
            "Press Tab in the search bar, or start with ?, to ask an AI agent. The agent \
             signs in with its own account and can answer, but never edits files or runs \
             commands.",
            theme,
        );

        let mut options = vec![
            self.render_option("off", Choice::Off, "Off".into(), "Quick Ask disabled".into(), cx)
                .into_any_element(),
        ];
        for (index, preset) in AGENT_PRESETS.iter().enumerate() {
            options.push(
                self.render_option(
                    preset.name,
                    Choice::Preset(index),
                    preset.name.into(),
                    preset.sign_in.into(),
                    cx,
                )
                .into_any_element(),
            );
        }
        options.push(
            self.render_option(
                "custom",
                Choice::Custom,
                "Custom command".into(),
                "Any ACP agent".into(),
                cx,
            )
            .into_any_element(),
        );

        let custom_fields = (self.choice == Choice::Custom).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(self.render_field("Name", &self.name, cx))
                .child(self.render_field("Command", &self.command, cx))
                .child(self.render_field("Arguments", &self.args, cx))
        });
        let models = self
            .agent(cx)
            .map(|agent| self.render_models(&agent, cx));

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::next_field))
            .on_key_down(cx.listener(Self::record_key))
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .font_family(theme::UI_FONT)
            .bg(surface)
            .text_color(text)
            .child(
                div()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(general)
                    .child(self.render_hotkey(cx))
                    .child(self.render_launch_at_login(cx))
                    .child(div().h(px(1.)).my_2().bg(border))
                    .child(search)
                    .child(self.render_engines(cx))
                    .child(div().h(px(1.)).my_2().bg(border))
                    .child(snippets)
                    .child(self.render_snippets(cx))
                    .child(div().h(px(1.)).my_2().bg(border))
                    .child(quick_ask)
                    .child(div().flex().flex_col().gap_1().children(options))
                    .children(custom_fields)
                    .children(models)
                    .child(self.render_test(cx)),
            )
    }
}
