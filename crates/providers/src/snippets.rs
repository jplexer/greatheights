//! Snippets: `;keyword` expands to text. A few are built in and computed
//! fresh each time (dates, UUIDs); the rest come from Settings.

use std::sync::RwLock;

use greatheights_core::{Action, Icon, Item, Matcher, Provider, Run, Snippet};

/// Snippet queries start with this, so snippets never crowd other results.
pub const PREFIX: char = ';';

/// Above apps and web results; a `;` query is clearly asking for a snippet.
const SCORE: u32 = 5_000;
const PREVIEW_CHARS: usize = 80;

const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.";

struct BuiltIn {
    keyword: &'static str,
    description: &'static str,
    expand: fn() -> String,
}

const BUILT_IN: &[BuiltIn] = &[
    BuiltIn {
        keyword: "date",
        description: "Today's date",
        expand: || chrono::Local::now().format("%Y-%m-%d").to_string(),
    },
    BuiltIn {
        keyword: "time",
        description: "The current time",
        expand: || chrono::Local::now().format("%H:%M").to_string(),
    },
    BuiltIn {
        keyword: "now",
        description: "ISO 8601 timestamp",
        expand: || chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
    },
    BuiltIn {
        keyword: "uuid",
        description: "A new random UUID",
        expand: || uuid::Uuid::new_v4().to_string(),
    },
    BuiltIn {
        keyword: "shrug",
        description: "Shrug",
        expand: || "¯\\_(ツ)_/¯".to_owned(),
    },
    BuiltIn {
        keyword: "lorem",
        description: "Placeholder text",
        expand: || LOREM.to_owned(),
    },
];

#[derive(Default)]
pub struct SnippetProvider {
    user: RwLock<Vec<Snippet>>,
}

impl SnippetProvider {
    pub fn new(snippets: Vec<Snippet>) -> Self {
        Self {
            user: RwLock::new(snippets),
        }
    }

    pub fn set_snippets(&self, snippets: Vec<Snippet>) {
        *self.user.write().unwrap() = snippets;
    }
}

impl Provider for SnippetProvider {
    fn search(&self, query: &str, _matcher: &mut Matcher) -> Vec<Item> {
        let Some(typed) = query.trim().strip_prefix(PREFIX) else {
            return Vec::new();
        };
        let typed = typed.trim().to_lowercase();
        let mut matcher = Matcher::new(&typed);
        let mut score = |keyword: &str| -> Option<u32> {
            if typed.is_empty() {
                return Some(0);
            }
            let keyword = keyword.to_lowercase();
            if keyword == typed {
                return Some(2_000);
            }
            if keyword.starts_with(&typed) {
                return Some(1_000);
            }
            matcher.score(&keyword)
        };

        let user = self.user.read().unwrap();
        // The user's snippets win over built-ins with the same keyword.
        // Equal matches keep the order snippets are listed in, the user's
        // own first.
        let order = |index: usize, offset: usize| (offset + 200).saturating_sub(index) as u32;
        let mut items: Vec<Item> = user
            .iter()
            .enumerate()
            .filter(|(_, snippet)| !snippet.keyword.trim().is_empty())
            .filter_map(|(index, snippet)| {
                let score = score(&snippet.keyword)?;
                Some(item(&snippet.keyword, &snippet.text, None, score + order(index, 100)))
            })
            .collect();
        for (index, built_in) in BUILT_IN.iter().enumerate() {
            if user.iter().any(|s| s.keyword.eq_ignore_ascii_case(built_in.keyword)) {
                continue;
            }
            if let Some(score) = score(built_in.keyword) {
                let text = (built_in.expand)();
                let score = score + order(index, 0);
                items.push(item(built_in.keyword, &text, Some(built_in.description), score));
            }
        }
        items
    }
}

fn item(keyword: &str, text: &str, description: Option<&str>, score: u32) -> Item {
    let preview: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview = if preview.chars().count() > PREVIEW_CHARS {
        format!("{}…", preview.chars().take(PREVIEW_CHARS).collect::<String>())
    } else {
        preview
    };
    let subtitle = match description {
        Some(description) => format!("{PREFIX}{keyword} · {description}"),
        None => format!("{PREFIX}{keyword}"),
    };
    Item {
        id: format!("snippet:{keyword}"),
        title: preview,
        subtitle: Some(subtitle),
        icon: Icon::Glyph("¶"),
        actions: vec![Action::new("Copy", Run::CopyText(text.to_owned()))],
        score: SCORE + score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(provider: &SnippetProvider, query: &str) -> Vec<Item> {
        let mut items = provider.search(query, &mut Matcher::new(query));
        items.sort_by(|a, b| b.score.cmp(&a.score));
        items
    }

    fn text(item: &Item) -> &str {
        match &item.actions[0].run {
            Run::CopyText(text) => text,
            other => panic!("not a copy: {other:?}"),
        }
    }

    #[test]
    fn needs_the_prefix() {
        assert!(search(&SnippetProvider::default(), "date").is_empty());
    }

    #[test]
    fn expands_built_ins_fresh() {
        let provider = SnippetProvider::default();
        let date = search(&provider, ";date").remove(0);
        assert_eq!(text(&date), chrono::Local::now().format("%Y-%m-%d").to_string());
        let a = text(&search(&provider, ";uuid")[0]).to_owned();
        let b = text(&search(&provider, ";uuid")[0]).to_owned();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
    }

    #[test]
    fn user_snippets_match_and_override() {
        let provider = SnippetProvider::new(vec![
            Snippet { keyword: "email".into(), text: "me@example.org".into() },
            Snippet { keyword: "shrug".into(), text: "meh".into() },
        ]);
        let email = search(&provider, ";em").remove(0);
        assert_eq!(text(&email), "me@example.org");
        assert_eq!(email.subtitle.as_deref(), Some(";email"));
        let shrugs = search(&provider, ";shrug");
        assert_eq!(shrugs.len(), 1);
        assert_eq!(text(&shrugs[0]), "meh");
    }

    #[test]
    fn exact_keyword_ranks_first() {
        let provider = SnippetProvider::new(vec![Snippet {
            keyword: "dates".into(),
            text: "x".into(),
        }]);
        assert_eq!(search(&provider, ";date")[0].subtitle.as_deref(), Some(";date · Today's date"));
    }

    #[test]
    fn bare_prefix_lists_everything_in_order() {
        let provider = SnippetProvider::new(vec![Snippet {
            keyword: "zz".into(),
            text: "mine".into(),
        }]);
        let keywords: Vec<_> = search(&provider, ";")
            .iter()
            .map(|item| item.id.trim_start_matches("snippet:").to_owned())
            .collect();
        assert_eq!(keywords, ["zz", "date", "time", "now", "uuid", "shrug", "lorem"]);
    }
}
