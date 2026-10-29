//! Web search: `!bang` shortcuts, with Startpage as the fallback.
//!
//! The bang list is Kagi's community-maintained one (MIT, see
//! `data/bangs.LICENSE`), bundled so it works offline. To update it, replace
//! `data/bangs.json` with the latest from github.com/kagisearch/bangs.

use std::collections::HashMap;
use std::sync::OnceLock;

use greatheights_core::{Action, Icon, Item, Matcher, Provider, Run};
use serde::Deserialize;

const BANGS_JSON: &str = include_str!("../data/bangs.json");

/// An explicit bang is the clearest intent there is, so it goes first.
const BANG_SCORE: u32 = 20_000;

const FALLBACK_NAME: &str = "Startpage";
const FALLBACK_URL: &str = "https://www.startpage.com/do/search?q={{{s}}}";

#[derive(Deserialize)]
struct Bang {
    /// Site name.
    s: String,
    /// Domain, opened when there are no search terms.
    d: String,
    /// Trigger.
    t: String,
    /// Extra triggers.
    #[serde(default)]
    ts: Vec<String>,
    /// URL template with a `{{{s}}}` placeholder.
    u: String,
    /// Formatting flags; absent means all of them.
    fmt: Option<Vec<String>>,
}

impl Bang {
    fn has_flag(&self, flag: &str) -> bool {
        self.fmt.as_ref().is_none_or(|fmt| fmt.iter().any(|f| f == flag))
    }

    fn url(&self, terms: &str) -> String {
        if terms.is_empty() && self.has_flag("open_base_path") {
            return format!("https://{}", self.d);
        }
        let terms = if self.has_flag("url_encode_placeholder") {
            encode(terms, self.has_flag("url_encode_space_to_plus"))
        } else {
            terms.to_owned()
        };
        self.u.replace("{{{s}}}", &terms)
    }
}

struct Bangs {
    list: Vec<Bang>,
    by_trigger: HashMap<String, usize>,
}

fn bangs() -> &'static Bangs {
    static BANGS: OnceLock<Bangs> = OnceLock::new();
    BANGS.get_or_init(|| {
        let list: Vec<Bang> = serde_json::from_str(BANGS_JSON).unwrap_or_else(|err| {
            log::error!("bundled bang list is invalid: {err}");
            Vec::new()
        });
        let mut by_trigger = HashMap::new();
        for (i, bang) in list.iter().enumerate() {
            for trigger in std::iter::once(&bang.t).chain(&bang.ts) {
                // The first bang to claim a trigger keeps it.
                by_trigger.entry(trigger.to_lowercase()).or_insert(i);
            }
        }
        Bangs { list, by_trigger }
    })
}

pub struct WebProvider;

impl WebProvider {
    pub fn new() -> Self {
        // Parse the bang list now, off the UI thread, rather than on first use.
        std::thread::spawn(bangs);
        Self
    }
}

impl Default for WebProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for WebProvider {
    fn search(&self, query: &str, _matcher: &mut Matcher) -> Vec<Item> {
        if let Some((bang, terms)) = find_bang(query) {
            let title = if terms.is_empty() {
                format!("Open {}", bang.s)
            } else {
                format!("Search {} for “{terms}”", bang.s)
            };
            return vec![web_item(title, &bang.d, bang.url(&terms), BANG_SCORE)];
        }

        let url = FALLBACK_URL.replace("{{{s}}}", &encode(query, true));
        let title = format!("Search {FALLBACK_NAME} for “{query}”");
        // Score 0 keeps the fallback below every real match.
        vec![web_item(title, "Web search", url, 0)]
    }
}

fn web_item(title: String, subtitle: &str, url: String, score: u32) -> Item {
    Item {
        // Not remembered: these echo the query, so they'd log search history.
        id: String::new(),
        title,
        subtitle: Some(subtitle.to_owned()),
        icon: Icon::Glyph("↗"),
        actions: vec![Action::new("Open in Browser", Run::OpenUrl(url))],
        score,
    }
}

/// Finds a `!trigger` word anywhere in the query, like DuckDuckGo and Kagi
/// do, and returns the bang with the remaining words as search terms.
fn find_bang(query: &str) -> Option<(&'static Bang, String)> {
    let bangs = bangs();
    let words: Vec<&str> = query.split_whitespace().collect();
    let (position, bang) = words.iter().enumerate().find_map(|(i, word)| {
        let trigger = word.strip_prefix('!')?.to_lowercase();
        let index = *bangs.by_trigger.get(&trigger)?;
        Some((i, &bangs.list[index]))
    })?;
    let terms = words
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != position)
        .map(|(_, word)| *word)
        .collect::<Vec<_>>()
        .join(" ");
    Some((bang, terms))
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
fn encode(text: &str, space_as_plus: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' if space_as_plus => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(query: &str) -> Item {
        WebProvider.search(query, &mut Matcher::new(query)).remove(0)
    }

    fn url(item: &Item) -> &str {
        match &item.actions[0].run {
            Run::OpenUrl(url) => url,
            other => panic!("not a URL: {other:?}"),
        }
    }

    #[test]
    fn bang_at_start_or_end() {
        let start = search("!g rust gpui");
        let end = search("rust gpui !G");
        assert_eq!(url(&start), "https://www.google.com/search?q=rust+gpui");
        assert_eq!(url(&start), url(&end));
        assert_eq!(start.score, BANG_SCORE);
    }

    #[test]
    fn alias_triggers_work() {
        assert_eq!(url(&search("!youtube lofi")), url(&search("!yt lofi")));
    }

    #[test]
    fn bare_bang_opens_the_site() {
        assert_eq!(url(&search("!gh")), "https://github.com");
    }

    #[test]
    fn falls_back_to_startpage() {
        let item = search("what is a bang?");
        assert_eq!(
            url(&item),
            "https://www.startpage.com/do/search?q=what+is+a+bang%3F"
        );
        assert_eq!(item.score, 0);
        assert!(item.id.is_empty());
    }

    #[test]
    fn unknown_bang_is_searched_literally() {
        assert!(url(&search("!notarealbang42 hi")).starts_with("https://www.startpage.com/"));
    }
}
