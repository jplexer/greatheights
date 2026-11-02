//! The Quick Ask answer, streamed in below the search bar. Follow-ups turn it
//! into a short chat, one turn per question.

use greatheights_ai::{Ask, Chat};
use gpui::{
    FontWeight, IntoElement, ParentElement, ScrollHandle, Styled, Task, div, prelude::*, px,
};

use crate::launcher::kbd;
use crate::markdown::{self, Block};
use crate::theme::Theme;

const PADDING: f32 = 12.;
const LINE_HEIGHT: f32 = 20.;
const FOOTER_HEIGHT: f32 = 18.;
const GAP: f32 = 10.;
/// Between a question and its answer.
const QUESTION_GAP: f32 = 4.;
/// Between turns.
const TURN_GAP: f32 = 16.;
/// Taller answers scroll.
const MAX_TEXT_HEIGHT: f32 = 400.;

pub enum Status {
    Waiting,
    Streaming,
    Done,
    Failed(String),
}

pub struct Turn {
    pub question: String,
    /// The raw Markdown, which is what Enter copies.
    pub text: String,
    /// `text`, parsed for display.
    pub blocks: Vec<Block>,
}

impl Turn {
    pub fn new(question: String) -> Self {
        Self {
            question,
            text: String::new(),
            blocks: Vec::new(),
        }
    }
}

pub struct Answer {
    pub agent: String,
    /// Oldest first; the last one is being answered.
    pub turns: Vec<Turn>,
    pub status: Status,
    /// The session follow-ups continue.
    pub chat: Chat,
    /// Whether the bar holds a follow-up rather than a search. Set once the
    /// chat is continued, for the rest of it.
    pub following_up: bool,
    /// Dropping the ask cancels it, so keep it until the answer finishes.
    pub ask: Option<Ask>,
    pub scroll: ScrollHandle,
    pub _task: Task<()>,
}

impl Answer {
    /// Height of the panel below the bar, divider included.
    pub fn height(&self) -> f32 {
        1. + PADDING * 2. + self.text_height() + GAP + FOOTER_HEIGHT
    }

    pub fn latest(&self) -> &Turn {
        self.turns.last().expect("an answer has a turn")
    }

    pub fn is_done(&self) -> bool {
        matches!(self.status, Status::Done)
    }

    /// Whether a follow-up can be sent now.
    pub fn can_follow_up(&self) -> bool {
        matches!(self.status, Status::Done | Status::Failed(_))
    }

    pub fn push_text(&mut self, text: &str) {
        let turn = self.turns.last_mut().expect("an answer has a turn");
        turn.text.push_str(text);
        turn.blocks = markdown::parse(&turn.text);
        self.scroll.scroll_to_bottom();
    }

    pub fn push_turn(&mut self, question: String, ask: Ask, task: Task<()>) {
        self.turns.push(Turn::new(question));
        self.status = Status::Waiting;
        self.chat = ask.chat.clone();
        self.ask = Some(ask);
        self._task = task;
        self.scroll.scroll_to_bottom();
    }

    /// The questions show once the bar no longer holds the first one.
    fn shows_questions(&self) -> bool {
        self.following_up || self.turns.len() > 1
    }

    fn text_height(&self) -> f32 {
        let questions = self.shows_questions();
        let turns: f32 = self
            .turns
            .iter()
            .map(|turn| {
                let answer = markdown::estimate_height(&turn.blocks).max(LINE_HEIGHT);
                if questions {
                    LINE_HEIGHT + QUESTION_GAP + answer
                } else {
                    answer
                }
            })
            .sum();
        let gaps = TURN_GAP * self.turns.len().saturating_sub(1) as f32;
        (turns + gaps).clamp(LINE_HEIGHT, MAX_TEXT_HEIGHT)
    }

    fn render_turn(&self, index: usize, theme: &Theme) -> gpui::Div {
        let turn = &self.turns[index];
        let latest = index + 1 == self.turns.len();
        let failure = match &self.status {
            Status::Failed(err) if latest => Some(err.clone()),
            _ => None,
        };
        let body = match (failure, turn.text.is_empty()) {
            // Nothing to show but the error: it takes the answer's place.
            (Some(err), true) => div().text_color(theme.error).child(err),
            (None, true) if latest => div()
                .text_color(theme.text_muted)
                .child(format!("Asking {}…", self.agent)),
            // An earlier turn that was cut short.
            (None, true) => div().text_color(theme.text_muted).child("No answer"),
            (_, false) => markdown::render(&turn.blocks, theme),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(QUESTION_GAP))
            .when(self.shows_questions(), |turn_box| {
                turn_box.child(
                    div()
                        .h(px(LINE_HEIGHT))
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(turn.question.clone()),
                )
            })
            .child(body)
    }

    /// `draft` says whether the bar holds a follow-up ready to send.
    pub fn render(&self, draft: bool, theme: &Theme) -> impl IntoElement {
        // A failure partway through keeps the partial answer and notes the
        // error below it.
        let status = match &self.status {
            Status::Failed(err) if !self.latest().text.is_empty() => Some(err.clone()),
            _ => None,
        };
        let (status, status_color) = match status {
            Some(err) => (err, theme.error),
            None => (self.agent.clone(), theme.text_muted),
        };
        let hint = |key, label| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(kbd(key, theme))
                .child(label)
        };
        let sendable = draft && self.can_follow_up();
        let hints = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .text_color(theme.text_muted)
            .when(sendable, |hints| hints.child(hint("↵", "Send")))
            .when(!sendable && self.is_done(), |hints| {
                hints.child(hint("↵", "Copy"))
            })
            .when(!self.following_up && self.can_follow_up(), |hints| {
                hints.child(hint("tab", "Continue"))
            })
            .child(hint("esc", "Close"));

        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(div().h(px(1.)).flex_none().bg(theme.border_variant))
            .child(
                div()
                    .p(px(PADDING))
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .child(
                        div()
                            .id("answer")
                            .h(px(self.text_height()))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .text_size(px(14.))
                            .line_height(px(LINE_HEIGHT))
                            .child(
                                div().flex().flex_col().gap(px(TURN_GAP)).children(
                                    (0..self.turns.len())
                                        .map(|index| self.render_turn(index, theme)),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(FOOTER_HEIGHT))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .text_size(px(12.))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(status_color)
                                    .child(status),
                            )
                            .child(hints),
                    ),
            )
    }
}
