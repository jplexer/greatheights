use std::ops::Range;
use std::sync::Arc;

use greatheights_core::{Engine, Icon, Item, Run};
use gpui::{
    App, ClipboardItem, Context, Div, Entity, FocusHandle, Focusable, KeyBinding, MouseMoveEvent, ScrollStrategy,
    SharedString,
    Pixels, Point, Stateful, Subscription, UniformListScrollHandle, Window, actions, div, img,
    prelude::*, px,
    uniform_list,
};

use crate::input::{InputChanged, TextInput};
use crate::theme::Theme;
use crate::{platform, window};

actions!(
    launcher,
    [SelectNext, SelectPrev, Confirm, ConfirmSecondary, Dismiss, Quit]
);

const CONTEXT: &str = "Launcher";

pub const BAR_HEIGHT: f32 = 56.;
const ROW_HEIGHT: f32 = 44.;
const ICON_SIZE: f32 = 28.;
const MAX_VISIBLE_ROWS: usize = 8;
const LIST_PADDING: f32 = 6.;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("ctrl-n", SelectNext, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
        KeyBinding::new("ctrl-p", SelectPrev, Some(CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("secondary-enter", ConfirmSecondary, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
        KeyBinding::new("secondary-q", Quit, Some(CONTEXT)),
    ]);
}

pub struct Launcher {
    input: Entity<TextInput>,
    engine: Arc<Engine>,
    results: Vec<Item>,
    selected: usize,
    /// The selected item's action awaiting a second Enter, if any.
    confirming: Option<usize>,
    scroll: UniformListScrollHandle,
    /// Where the pointer was last seen, so rows moving under a still pointer
    /// don't steal the selection from the keyboard.
    last_mouse: Option<Point<Pixels>>,
    _subscriptions: Vec<Subscription>,
}

impl Launcher {
    pub fn new(engine: Arc<Engine>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Search…", cx));
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, _: &InputChanged, window, cx| {
                this.update_results(window, cx);
            }),
            // Clicking elsewhere dismisses the launcher, like Spotlight.
            cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    cx.defer(|cx| {
                        window::hide(cx);
                    });
                }
            }),
        ];
        Self {
            input,
            engine,
            results: Vec::new(),
            selected: 0,
            confirming: None,
            scroll: UniformListScrollHandle::new(),
            last_mouse: None,
            _subscriptions: subscriptions,
        }
    }

    /// Clears the query and results, ready for the next show.
    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.results.clear();
        self.selected = 0;
        self.confirming = None;
        self.last_mouse = None;
        platform::set_content_height(window, BAR_HEIGHT, cx);
    }

    #[cfg(debug_assertions)]
    pub fn set_query(&mut self, query: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text(query, cx));
    }

    fn update_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_owned();
        self.results = self.engine.search(&query);
        self.selected = 0;
        self.confirming = None;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        platform::set_content_height(window, content_height(self.results.len()), cx);
        cx.notify();
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.selected {
            self.selected = index;
            self.confirming = None;
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
            cx.notify();
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let last = self.results.len().saturating_sub(1);
        self.select((self.selected + 1).min(last), cx);
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.saturating_sub(1), cx);
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.run_action(0, cx);
    }

    fn confirm_secondary(&mut self, _: &ConfirmSecondary, _: &mut Window, cx: &mut Context<Self>) {
        self.run_action(1, cx);
    }

    fn run_action(&mut self, action: usize, cx: &mut Context<Self>) {
        let Some(item) = self.results.get(self.selected) else {
            return;
        };
        let Some(chosen) = item.actions.get(action) else {
            return;
        };
        if chosen.confirm && self.confirming != Some(action) {
            self.confirming = Some(action);
            cx.notify();
            return;
        }
        self.confirming = None;
        let run = chosen.run.clone();
        match run {
            Run::CopyText(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.engine.record(item);
            }
            Run::Quit => {
                cx.quit();
                return;
            }
            _ => self.engine.execute(item, action),
        }
        cx.defer(|cx| {
            window::hide(cx);
        });
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.defer(|cx| {
            window::hide(cx);
        });
    }

    fn quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.global::<Theme>();
        let item = &self.results[index];
        let icon = match &item.icon {
            Icon::Image(path) => img(path.clone()).size(px(ICON_SIZE)).into_any_element(),
            Icon::Glyph(glyph) => div()
                .size(px(ICON_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .bg(theme.row_selected)
                .text_size(px(16.))
                .text_color(theme.text_muted)
                .child(*glyph)
                .into_any_element(),
            Icon::None => div().size(px(ICON_SIZE)).into_any_element(),
        };

        let confirming = index == self.selected && self.confirming.is_some();
        let subtitle = if confirming {
            Some("Press ↵ again to confirm".to_owned())
        } else {
            item.subtitle.clone()
        };

        div()
            .id(index)
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(8.))
            .when(index == self.selected, |row| row.bg(theme.row_selected))
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(15.))
                    .child(item.title.clone()),
            )
            .children(subtitle.map(|subtitle| {
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(if confirming { theme.accent } else { theme.text_muted })
                    .child(subtitle)
            }))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let moved = this.last_mouse.is_some_and(|last| last != event.position);
                this.last_mouse = Some(event.position);
                if moved {
                    this.select(index, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select(index, cx);
                this.run_action(0, cx);
            }))
    }
}

fn list_height(results: usize) -> f32 {
    LIST_PADDING * 2. + results.min(MAX_VISIBLE_ROWS) as f32 * ROW_HEIGHT
}

fn content_height(results: usize) -> f32 {
    if results == 0 {
        BAR_HEIGHT
    } else {
        BAR_HEIGHT + 1. + list_height(results)
    }
}

impl Focusable for Launcher {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();

        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::confirm_secondary))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::quit))
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .font_family(platform::UI_FONT)
            .bg(theme.background)
            .text_color(theme.text)
            .child(
                div()
                    .h(px(BAR_HEIGHT))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .text_size(px(22.))
                    .line_height(px(28.))
                    .child(self.input.clone()),
            )
            .when(!self.results.is_empty(), |root| {
                root.child(div().h(px(1.)).flex_none().bg(theme.border))
                    .child(
                        // A fixed height, so the frame drawn before the window
                        // finishes resizing is clipped rather than squashed.
                        div()
                            .flex_none()
                            .h(px(list_height(self.results.len())))
                            .p(px(LIST_PADDING))
                            .child(
                                uniform_list(
                                    "results",
                                    self.results.len(),
                                    cx.processor(|this, range: Range<usize>, _, cx| {
                                        range
                                            .map(|index| this.render_row(index, cx))
                                            .collect::<Vec<_>>()
                                    }),
                                )
                                .track_scroll(self.scroll.clone())
                                .size_full(),
                            ),
                    )
            })
    }
}
