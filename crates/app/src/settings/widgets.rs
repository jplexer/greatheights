//! The building blocks of the settings window, after Zed's settings UI:
//! grouped rows with a label on the left and a control on the right.

use gpui::{
    AnyElement, App, Context, Div, ElementId, Entity, Focusable, FontWeight, IntoElement, MouseButton,
    ParentElement, Render, SharedString, Stateful, Styled, Window, div, prelude::*, px,
};

use crate::input::TextInput;
use crate::theme::{self, Theme};

pub const CONTROL_HEIGHT: f32 = 28.;

/// A titled box of rows, separated by dividers.
pub fn group(
    title: &'static str,
    description: Option<SharedString>,
    rows: Vec<AnyElement>,
    theme: &Theme,
) -> Div {
    let count = rows.len();
    let rows = rows.into_iter().enumerate().map(|(index, row)| {
        div()
            .when(index + 1 < count, |row| {
                row.border_b_1().border_color(theme.border_variant)
            })
            .child(row)
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .children(description.map(|description| {
                    div()
                        .text_size(px(12.))
                        .text_color(theme.text_muted)
                        .child(description)
                })),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border_variant)
                .bg(theme.background)
                .children(rows),
        )
}

/// The line under a setting's name.
pub enum Note {
    Muted(SharedString),
    Error(SharedString),
}

/// A setting: its name and a note on the left, `control` on the right.
pub fn row(
    title: impl Into<SharedString>,
    note: Note,
    control: impl IntoElement,
    theme: &Theme,
) -> Div {
    let (note, color) = match note {
        Note::Muted(text) => (text, theme.text_muted),
        Note::Error(text) => (text, theme.error),
    };
    div()
        .min_h(px(48.))
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(13.)).child(title.into()))
                .child(div().text_size(px(12.)).text_color(color).child(note)),
        )
        .child(div().flex_none().child(control))
}

/// A row with free-form content, padded like [`row`].
pub fn plain_row() -> Div {
    div().px_3().py_2().flex().items_center().gap_2()
}

/// A row holding only a note, for empty lists.
pub fn note_row(text: impl Into<SharedString>, theme: &Theme) -> Div {
    plain_row()
        .min_h(px(40.))
        .text_size(px(12.))
        .text_color(theme.text_muted)
        .child(text.into())
}

pub fn switch(on: bool, theme: &Theme) -> Div {
    let knob = div()
        .size(px(14.))
        .rounded_full()
        .bg(if on { theme.background } else { theme.text_muted });
    div()
        .w(px(32.))
        .h(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(1.))
        .rounded_full()
        .border_1()
        .map(|track| {
            if on {
                track.bg(theme.accent).border_color(theme.accent).justify_end()
            } else {
                track.bg(theme.field).border_color(theme.border)
            }
        })
        .child(knob)
}

pub fn radio(selected: bool, theme: &Theme) -> Div {
    div()
        .size(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(if selected { theme.accent } else { theme.border })
        .bg(theme.field)
        .when(selected, |ring| {
            ring.child(div().size(px(8.)).rounded_full().bg(theme.accent))
        })
}

/// A clickable row with a radio button, for choosing one of a list.
pub fn radio_row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    detail: Option<SharedString>,
    selected: bool,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .min_h(px(40.))
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_3()
        .when(!selected, |row| row.hover(|row| row.bg(theme.row_hover)))
        .child(radio(selected, theme))
        .child(div().flex_1().min_w_0().text_size(px(13.)).child(title.into()))
        .children(detail.map(|detail| {
            div()
                .flex_none()
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child(detail)
        }))
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(CONTROL_HEIGHT))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.field)
        .text_size(px(13.))
        .hover(|button| button.bg(theme.row_hover))
        .child(label.into())
}

/// A small borderless button, for removing a list entry.
pub fn icon_button(id: impl Into<ElementId>, glyph: &'static str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(CONTROL_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_size(px(14.))
        .text_color(theme.text_muted)
        .hover(|button| button.bg(theme.row_hover).text_color(theme.text))
        .child(glyph)
}

/// One option of a segmented control.
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(CONTROL_HEIGHT - 6.))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(3.))
        .text_size(px(13.))
        .map(|segment| {
            if selected {
                segment.bg(theme.row_selected)
            } else {
                segment
                    .text_color(theme.text_muted)
                    .hover(|segment| segment.bg(theme.row_hover).text_color(theme.text))
            }
        })
        .child(label.into())
}

pub fn segmented(segments: impl IntoIterator<Item = Stateful<Div>>, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(5.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.field)
        .children(segments)
}

/// A selectable chip, for short lists of choices that wrap.
pub fn pill(
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

/// A bordered box around a text input, like Zed's input fields. `invalid`
/// outlines it in the error color.
pub fn input_box(input: &Entity<TextInput>, invalid: bool, theme: &Theme, cx: &App) -> Div {
    let focus = input.focus_handle(cx);
    div()
        .min_w_0()
        .overflow_hidden()
        .h(px(CONTROL_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(theme.field)
        .border_1()
        .border_color(if invalid { theme.error } else { theme.border })
        .font_family(theme::MONO_FONT)
        .text_size(px(13.))
        .line_height(px(18.))
        .on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
        .child(input.clone())
}

/// A label and input on one line, for small forms inside a group.
pub fn field_row(
    label: &'static str,
    input: &Entity<TextInput>,
    invalid: bool,
    theme: &Theme,
    cx: &App,
) -> Div {
    plain_row()
        .gap_3()
        .child(
            div()
                .w(px(84.))
                .flex_none()
                .text_size(px(13.))
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(input_box(input, invalid, theme, cx).flex_1())
}

/// A plain text tooltip.
pub struct Tooltip(pub SharedString);

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
