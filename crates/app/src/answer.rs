//! The Quick Ask answer, streamed in below the search bar.

use greatheights_ai::Ask;
use gpui::{IntoElement, ParentElement, Styled, Task, div, prelude::*, px};

use crate::theme::Theme;

const PADDING: f32 = 16.;
const LINE_HEIGHT: f32 = 20.;
const FOOTER_HEIGHT: f32 = 18.;
const GAP: f32 = 10.;
/// Taller answers scroll.
const MAX_TEXT_HEIGHT: f32 = 360.;
/// Roughly how many characters of 14px system text fit on a line of the
/// answer panel; used to size the window before layout.
const CHARS_PER_LINE: usize = 90;

pub enum Status {
    Waiting,
    Streaming,
    Done,
    Failed(String),
}

pub struct Answer {
    pub agent: String,
    pub text: String,
    pub status: Status,
    /// Dropping the ask cancels it, so keep it until the answer finishes.
    pub ask: Option<Ask>,
    pub _task: Task<()>,
}

impl Answer {
    /// Height of the panel below the bar, divider included.
    pub fn height(&self) -> f32 {
        1. + PADDING * 2. + self.text_height() + GAP + FOOTER_HEIGHT
    }

    fn text_height(&self) -> f32 {
        let lines: usize = self
            .text
            .trim_end()
            .lines()
            .map(|line| line.chars().count().div_ceil(CHARS_PER_LINE).max(1))
            .sum();
        (lines.max(1) as f32 * LINE_HEIGHT).min(MAX_TEXT_HEIGHT)
    }

    pub fn render(&self, theme: &Theme) -> impl IntoElement {
        let failure = match &self.status {
            Status::Failed(err) => Some(err.clone()),
            _ => None,
        };
        let body = match (&failure, self.text.is_empty()) {
            // Nothing to show but the error: it takes the answer's place.
            (Some(err), true) => div().text_color(theme.error).child(err.clone()),
            (None, true) => div()
                .text_color(theme.text_muted)
                .child(format!("Asking {}…", self.agent)),
            (_, false) => div().child(self.text.trim_end().to_owned()),
        };
        // A failure partway through keeps the partial answer and notes the
        // error below it.
        let (status, status_color) = match failure.filter(|_| !self.text.is_empty()) {
            Some(err) => (err, theme.error),
            None => (self.agent.clone(), theme.text_muted),
        };
        let hints = match self.status {
            Status::Done => "↵ Copy   esc Close",
            _ => "esc Close",
        };

        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(div().h(px(1.)).flex_none().bg(theme.border))
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
                            .text_size(px(14.))
                            .line_height(px(LINE_HEIGHT))
                            .child(body),
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
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(theme.text_muted)
                                    .child(hints),
                            ),
                    ),
            )
    }
}
