//! The settings window. Every change is saved immediately.

use std::time::Duration;

use greatheights_ai::{Ask, AskEvent};
use greatheights_core::{AGENT_PRESETS, AgentConfig, Settings};
use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, Global, KeyBinding,
    MouseButton, SharedString, Subscription, Task, TitlebarOptions, Window, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, actions, div, prelude::*, px, size,
};

use crate::input::{InputChanged, TextInput};
use crate::platform;
use crate::theme::Theme;
use crate::window::{AskerHandle, Config};

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
            // Tall enough for the custom agent fields.
            size(px(520.), px(590.)),
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
        is_resizable: false,
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

pub struct SettingsView {
    focus: FocusHandle,
    choice: Choice,
    name: Entity<TextInput>,
    command: Entity<TextInput>,
    args: Entity<TextInput>,
    test: TestState,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let agent = cx.global::<Config>().settings.agent.clone();
        let choice = match &agent {
            None => Choice::Off,
            Some(agent) => AGENT_PRESETS
                .iter()
                .position(|preset| preset.config().same_launch(agent))
                .map_or(Choice::Custom, Choice::Preset),
        };
        // Custom fields start from the configured custom agent, if any.
        let custom = agent.filter(|_| choice == Choice::Custom);
        let field = |placeholder: &str, text: String, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(placeholder.to_owned(), cx);
                input.set_text(text, cx);
                input
            })
        };
        let name = field("My Agent", custom.as_ref().and_then(|a| a.name.clone()).unwrap_or_default(), cx);
        let command = field("my-agent", custom.as_ref().map(|a| a.command.clone()).unwrap_or_default(), cx);
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
        Self {
            focus,
            choice,
            name,
            command,
            args,
            test: TestState::Idle,
            _subscriptions: subscriptions,
        }
    }

    fn agent(&self, cx: &App) -> Option<AgentConfig> {
        match self.choice {
            Choice::Off => None,
            Choice::Preset(index) => Some(AGENT_PRESETS[index].config()),
            Choice::Custom => {
                let command = self.command.read(cx).text().trim().to_owned();
                if command.is_empty() {
                    return None;
                }
                let name = self.name.read(cx).text().trim().to_owned();
                Some(AgentConfig {
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
                })
            }
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let settings = Settings {
            agent: self.agent(cx),
        };
        let path = cx.global::<Config>().path.clone();
        if let Err(err) = settings.save(&path) {
            log::error!("couldn't save settings to {}: {err}", path.display());
        }
        cx.set_global(Config { path, settings });
        // A previous result says nothing about the new agent.
        self.test = TestState::Idle;
        cx.notify();
    }

    fn choose(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        if choice == self.choice {
            return;
        }
        self.choice = choice;
        self.save(cx);
        if choice == Choice::Custom {
            window.focus(&self.command.focus_handle(cx));
        }
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

    fn close(&mut self, _: &Close, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    fn next_field(&mut self, _: &NextField, window: &mut Window, cx: &mut Context<Self>) {
        if self.choice != Choice::Custom {
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
            .rounded(px(8.))
            .when(selected, |row| row.bg(theme.row_selected))
            .hover(|row| row.bg(theme.row_selected))
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
        let focus = input.focus_handle(cx);
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .w(px(84.))
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .h(px(30.))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .bg(theme.field)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
                    .child(input.clone()),
            )
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
                    .rounded(px(6.))
                    .bg(theme.field)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(13.))
                    .when(!can_test, |button| button.text_color(theme.text_muted))
                    .when(can_test, |button| {
                        button
                            .hover(|button| button.bg(theme.row_selected))
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

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let (text, muted, surface) = (theme.text, theme.text_muted, theme.surface);

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

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::next_field))
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .font_family(platform::UI_FONT)
            .bg(surface)
            .text_color(text)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_size(px(17.)).child("Quick Ask"))
                    .child(div().text_size(px(13.)).text_color(muted).child(
                        "Press Tab in the search bar, or start with ?, to ask an AI agent. \
                         The agent signs in with its own account and can answer, but never \
                         edits files or runs commands.",
                    )),
            )
            .child(div().flex().flex_col().gap_1().children(options))
            .children(custom_fields)
            .child(self.render_test(cx))
    }
}
