//! Just enough Markdown for Quick Ask answers: headings, paragraphs, lists,
//! quotes, code blocks, rules, and bold, italic, code, links and
//! strikethrough inline. Parsing is cheap enough to redo on every streamed
//! chunk, and half-written Markdown renders sensibly.

use std::ops::Range;

use gpui::{
    Div, FontStyle, FontWeight, HighlightStyle, IntoElement, ParentElement, StrikethroughStyle,
    Styled, StyledText, UnderlineStyle, div, px,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::theme::{self, Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Span {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: bool,
    pub strike: bool,
}

/// Text with styled ranges.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<(Range<usize>, Span)>,
}

impl Inline {
    fn push(&mut self, text: &str, span: Span) {
        let start = self.text.len();
        self.text.push_str(text);
        if span != Span::default() {
            self.spans.push((start..self.text.len(), span));
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, Inline),
    Paragraph(Inline),
    ListItem {
        marker: String,
        depth: usize,
        content: Inline,
    },
    Quote(Inline),
    Code(String),
    Rule,
}

pub fn parse(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut inline = Inline::default();
    let mut style = Span::default();
    // One counter per open list; `None` for bullet lists.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut items = 0usize;
    let mut quote = 0usize;
    let mut code: Option<String> = None;

    let flush = |inline: &mut Inline, blocks: &mut Vec<Block>, quote: usize| {
        if inline.text.trim().is_empty() {
            *inline = Inline::default();
            return;
        }
        let content = std::mem::take(inline);
        blocks.push(if quote > 0 {
            Block::Quote(content)
        } else {
            Block::Paragraph(content)
        });
    };

    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Heading { .. }) | Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Heading(level)) => {
                blocks.push(Block::Heading(level as u8, std::mem::take(&mut inline)));
            }
            Event::End(TagEnd::Paragraph) => {
                if items > 0 {
                    // Loose list items hold paragraphs; keep them in the item.
                    inline.text.push('\n');
                } else {
                    flush(&mut inline, &mut blocks, quote);
                }
            }
            Event::Start(Tag::List(start)) => {
                // Text of an enclosing item belongs before its nested list.
                if items > 0 && !inline.text.trim().is_empty() {
                    push_item(&mut blocks, &mut lists, &mut inline);
                }
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => items += 1,
            Event::End(TagEnd::Item) => {
                if !inline.text.trim().is_empty() {
                    push_item(&mut blocks, &mut lists, &mut inline);
                }
                inline = Inline::default();
                items -= 1;
            }
            Event::Start(Tag::BlockQuote(_)) => quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut inline, &mut blocks, quote);
                quote -= 1;
            }
            Event::Start(Tag::CodeBlock(_)) => code = Some(String::new()),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(text) = code.take() {
                    blocks.push(Block::Code(text.trim_end_matches('\n').to_owned()));
                }
            }
            Event::Start(Tag::Emphasis) => style.italic = true,
            Event::End(TagEnd::Emphasis) => style.italic = false,
            Event::Start(Tag::Strong) => style.bold = true,
            Event::End(TagEnd::Strong) => style.bold = false,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::Start(Tag::Link { .. }) => style.link = true,
            Event::End(TagEnd::Link) => style.link = false,
            Event::Text(text) => match &mut code {
                Some(code) => code.push_str(&text),
                None => inline.push(&text, style),
            },
            Event::Code(text) => inline.push(&text, Span { code: true, ..style }),
            Event::Html(text) | Event::InlineHtml(text) => inline.push(&text, style),
            Event::SoftBreak => inline.push(" ", style),
            Event::HardBreak => inline.push("\n", style),
            Event::Rule => blocks.push(Block::Rule),
            _ => {}
        }
    }
    // Streaming answers often end mid-block.
    if let Some(text) = code {
        blocks.push(Block::Code(text.trim_end_matches('\n').to_owned()));
    }
    flush(&mut inline, &mut blocks, quote);
    blocks
}

fn push_item(blocks: &mut Vec<Block>, lists: &mut [Option<u64>], inline: &mut Inline) {
    let depth = lists.len().saturating_sub(1);
    let marker = match lists.last_mut() {
        Some(Some(number)) => {
            *number += 1;
            format!("{}.", *number - 1)
        }
        _ => "•".to_owned(),
    };
    let mut content = std::mem::take(inline);
    content.text.truncate(content.text.trim_end().len());
    blocks.push(Block::ListItem {
        marker,
        depth,
        content,
    });
}

const LINE_HEIGHT: f32 = 20.;
const CODE_LINE_HEIGHT: f32 = 18.;
const GAP: f32 = 8.;
/// Characters per line of answer text, and of code, in the 640px panel.
const CHARS_PER_LINE: usize = 80;
const CODE_CHARS_PER_LINE: usize = 70;

/// Height of the rendered blocks, estimated before layout to size the
/// window.
pub fn estimate_height(blocks: &[Block]) -> f32 {
    let lines = |text: &str, per_line: usize| -> f32 {
        text.lines()
            .map(|line| line.chars().count().div_ceil(per_line).max(1))
            .sum::<usize>()
            .max(1) as f32
    };
    let content: f32 = blocks
        .iter()
        .map(|block| match block {
            Block::Heading(_, inline) => lines(&inline.text, CHARS_PER_LINE) * 22.,
            Block::Paragraph(inline) | Block::Quote(inline) => {
                lines(&inline.text, CHARS_PER_LINE) * LINE_HEIGHT
            }
            Block::ListItem { depth, content, .. } => {
                lines(&content.text, CHARS_PER_LINE - 4 - depth * 4) * LINE_HEIGHT
            }
            Block::Code(text) => lines(text, CODE_CHARS_PER_LINE) * CODE_LINE_HEIGHT + 16.,
            Block::Rule => 1.,
        })
        .sum();
    content + GAP * blocks.len().saturating_sub(1) as f32
}

pub fn render(blocks: &[Block], theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .children(blocks.iter().map(|block| render_block(block, theme)))
}

fn render_block(block: &Block, theme: &Theme) -> impl IntoElement {
    match block {
        Block::Heading(level, inline) => div()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(if *level <= 2 { 16. } else { 14. }))
            .line_height(px(22.))
            .child(styled(inline, theme)),
        Block::Paragraph(inline) => div().child(styled(inline, theme)),
        Block::ListItem {
            marker,
            depth,
            content,
        } => div()
            .flex()
            .gap_2()
            .pl(px(*depth as f32 * 16.))
            .child(
                div()
                    .flex_none()
                    .min_w(px(14.))
                    .text_color(theme.text_muted)
                    .child(marker.clone()),
            )
            .child(div().flex_1().min_w_0().child(styled(content, theme))),
        Block::Quote(inline) => div()
            .border_l_2()
            .border_color(theme.border)
            .pl_3()
            .text_color(theme.text_muted)
            .child(styled(inline, theme)),
        Block::Code(text) => div()
            .px_3()
            .py_2()
            .rounded(px(4.))
            .bg(theme.field)
            .border_1()
            .border_color(theme.border_variant)
            .font_family(theme::MONO_FONT)
            .text_size(px(13.))
            .line_height(px(CODE_LINE_HEIGHT))
            .child(text.clone()),
        Block::Rule => div().h(px(1.)).bg(theme.border_variant),
    }
}

fn styled(inline: &Inline, theme: &Theme) -> StyledText {
    let highlights = inline.spans.iter().map(|(range, span)| {
        let mut style = HighlightStyle::default();
        if span.bold {
            style.font_weight = Some(FontWeight::SEMIBOLD);
        }
        if span.italic {
            style.font_style = Some(FontStyle::Italic);
        }
        if span.code {
            style.background_color = Some(theme.field);
        }
        if span.link {
            style.color = Some(theme.accent);
            style.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(theme.accent),
                wavy: false,
            });
        }
        if span.strike {
            style.strikethrough = Some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            });
        }
        (range.clone(), style)
    });
    StyledText::new(inline.text.clone()).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(block: &Block) -> &str {
        match block {
            Block::Heading(_, inline) | Block::Paragraph(inline) | Block::Quote(inline) => {
                &inline.text
            }
            Block::ListItem { content, .. } => &content.text,
            Block::Code(text) => text,
            Block::Rule => "---",
        }
    }

    #[test]
    fn parses_blocks() {
        let blocks = parse(
            "# Monads\n\nA **monad** wraps `values`.\n\n- one\n- two\n\n1. first\n2. second\n\n\
             ```rust\nlet x = Some(1);\n```\n\n> quoted\n\n---",
        );
        assert_eq!(
            blocks.iter().map(text).collect::<Vec<_>>(),
            [
                "Monads",
                "A monad wraps values.",
                "one",
                "two",
                "first",
                "second",
                "let x = Some(1);",
                "quoted",
                "---"
            ]
        );
        let Block::ListItem { marker, .. } = &blocks[5] else {
            panic!("not a list item")
        };
        assert_eq!(marker, "2.");
    }

    #[test]
    fn styles_inline_text() {
        let blocks = parse("A **bold** `code` [link](https://x.org) ~~old~~");
        let Block::Paragraph(inline) = &blocks[0] else {
            panic!("not a paragraph")
        };
        let styled: Vec<_> = inline
            .spans
            .iter()
            .map(|(range, span)| (&inline.text[range.clone()], *span))
            .collect();
        assert_eq!(styled[0], ("bold", Span { bold: true, ..Default::default() }));
        assert_eq!(styled[1], ("code", Span { code: true, ..Default::default() }));
        assert_eq!(styled[2], ("link", Span { link: true, ..Default::default() }));
        assert_eq!(styled[3], ("old", Span { strike: true, ..Default::default() }));
    }

    #[test]
    fn nested_lists_are_indented() {
        let blocks = parse("- outer\n  - inner\n- next");
        let depths: Vec<_> = blocks
            .iter()
            .map(|block| match block {
                Block::ListItem { depth, content, .. } => (content.text.as_str(), *depth),
                _ => panic!("expected list items, got {block:?}"),
            })
            .collect();
        assert_eq!(depths, [("outer", 0), ("inner", 1), ("next", 0)]);
    }

    #[test]
    fn renders_unfinished_code_blocks() {
        let blocks = parse("Try:\n\n```\nfn main() {");
        assert_eq!(blocks.last(), Some(&Block::Code("fn main() {".into())));
    }
}
