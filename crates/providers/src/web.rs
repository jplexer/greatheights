//! Web search: `!bang` shortcuts, falling back to the chosen search engine.
//!
//! The bang list is Kagi's community-maintained one (MIT, see
//! `data/bangs.LICENSE`), bundled so it works offline. To update it, replace
//! `data/bangs.json` with the latest from github.com/kagisearch/bangs.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use greatheights_core::{
    Action, CustomBang, Icon, Item, Matcher, Provider, Run, Settings, WebSearch, template_error,
};
use serde::Deserialize;

const BANGS_JSON: &str = include_str!("../data/bangs.json");

/// An explicit bang is the clearest intent there is, so it goes first.
const BANG_SCORE: u32 = 20_000;

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

pub struct WebProvider {
    config: RwLock<Config>,
}

struct Config {
    search: WebSearch,
    fallback: bool,
    /// Keyed by lowercase trigger.
    bangs: HashMap<String, CustomBang>,
}

impl WebProvider {
    pub fn new(settings: &Settings) -> Self {
        // Parse the bang list now, off the UI thread, rather than on first use.
        std::thread::spawn(bangs);
        Self {
            config: RwLock::new(Config::from(settings)),
        }
    }
}

impl Config {
    fn from(settings: &Settings) -> Self {
        let bangs = settings
            .bangs
            .iter()
            .filter(|bang| template_error(&bang.url).is_none())
            .filter_map(|bang| {
                let trigger = bang.trigger.trim().trim_start_matches('!').to_lowercase();
                (!trigger.is_empty()).then(|| (trigger, bang.clone()))
            })
            .collect();
        Self {
            search: settings.web_search(),
            fallback: settings.web_fallback,
            bangs,
        }
    }
}

impl Provider for WebProvider {
    fn search(&self, query: &str, _matcher: &mut Matcher) -> Vec<Item> {
        let config = self.config.read().unwrap();
        if let Some((bang, terms)) = find_custom_bang(query, &config.bangs) {
            let site = site(&bang.url);
            let title = if terms.is_empty() {
                format!("Open {site}")
            } else {
                format!("Search {site} for “{terms}”")
            };
            let url = if terms.is_empty() {
                origin(&bang.url).to_owned()
            } else {
                bang.url.trim().replace("%s", &encode(&terms, false))
            };
            return vec![web_item(title, &format!("!{}", bang.trigger.trim()), url, BANG_SCORE)];
        }
        if let Some((bang, terms)) = find_bang(query) {
            let title = if terms.is_empty() {
                format!("Open {}", bang.s)
            } else {
                format!("Search {} for “{terms}”", bang.s)
            };
            return vec![web_item(title, &bang.d, bang.url(&terms), BANG_SCORE)];
        }
        if !config.fallback {
            return Vec::new();
        }

        let search = &config.search;
        let url = search.template.replace("%s", &encode(query, true));
        let title = format!("Search {} for “{query}”", search.name);
        // Score 0 keeps the fallback below every real match.
        vec![web_item(title, "Web search", url, 0)]
    }

    fn configure(&self, settings: &Settings) {
        *self.config.write().unwrap() = Config::from(settings);
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

fn find_bang(query: &str) -> Option<(&'static Bang, String)> {
    let bangs = bangs();
    split_bang(query, |trigger| {
        bangs.by_trigger.get(trigger).map(|&index| &bangs.list[index])
    })
}

fn find_custom_bang<'a>(
    query: &str,
    bangs: &'a HashMap<String, CustomBang>,
) -> Option<(&'a CustomBang, String)> {
    if bangs.is_empty() {
        return None;
    }
    split_bang(query, |trigger| bangs.get(trigger))
}

/// Finds a `!trigger` word anywhere in the query, like DuckDuckGo and Kagi
/// do, and returns its bang with the remaining words as search terms.
fn split_bang<T>(query: &str, lookup: impl Fn(&str) -> Option<T>) -> Option<(T, String)> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let (position, bang) = words.iter().enumerate().find_map(|(i, word)| {
        let trigger = word.strip_prefix('!')?.to_lowercase();
        Some((i, lookup(&trigger)?))
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

/// `https://host` of a URL, opened by a bang without search terms.
fn origin(url: &str) -> &str {
    let url = url.trim();
    let host_start = url.find("://").map_or(0, |i| i + 3);
    match url[host_start..].find('/') {
        Some(end) => &url[..host_start + end],
        None => url,
    }
}

/// A URL's host without `www.`, to name a custom bang's site.
fn site(url: &str) -> &str {
    let origin = origin(url);
    let host = origin.split_once("://").map_or(origin, |(_, host)| host);
    host.strip_prefix("www.").unwrap_or(host)
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
    use greatheights_core::{CustomEngine, SearchEngine};

    use super::*;

    fn search(query: &str) -> Item {
        search_with(&Settings::default(), query).remove(0)
    }

    fn search_with(settings: &Settings, query: &str) -> Vec<Item> {
        WebProvider::new(settings).search(query, &mut Matcher::new(query))
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
    fn falls_back_to_the_chosen_engine() {
        let web = WebProvider::new(&Settings::default());
        web.configure(&Settings {
            search_engine: SearchEngine::Kagi,
            ..Settings::default()
        });
        let item = web.search("gpui", &mut Matcher::new("gpui")).remove(0);
        assert_eq!(url(&item), "https://kagi.com/search?q=gpui");
        assert_eq!(item.title, "Search Kagi for “gpui”");
    }

    #[test]
    fn falls_back_to_a_custom_engine() {
        let settings = Settings {
            search_engine: SearchEngine::Custom,
            custom_engine: CustomEngine {
                name: "Searx".into(),
                url: "https://searx.example/search?q=%s&lang=en".into(),
            },
            ..Settings::default()
        };
        let item = search_with(&settings, "a b").remove(0);
        assert_eq!(url(&item), "https://searx.example/search?q=a+b&lang=en");
        assert_eq!(item.title, "Search Searx for “a b”");
    }

    #[test]
    fn fallback_can_be_turned_off_but_bangs_still_work() {
        let settings = Settings {
            web_fallback: false,
            ..Settings::default()
        };
        assert!(search_with(&settings, "hello").is_empty());
        assert_eq!(search_with(&settings, "!gh").len(), 1);
    }

    #[test]
    fn custom_bangs_override_bundled_ones() {
        let settings = Settings {
            bangs: vec![CustomBang {
                trigger: "!G".into(),
                url: "https://kagi.com/search?q=%s".into(),
            }],
            ..Settings::default()
        };
        let item = search_with(&settings, "rust gpui !g").remove(0);
        assert_eq!(url(&item), "https://kagi.com/search?q=rust%20gpui");
        assert_eq!(item.title, "Search kagi.com for “rust gpui”");
        let bare = search_with(&settings, "!g").remove(0);
        assert_eq!(url(&bare), "https://kagi.com");
        // Other bundled bangs are untouched.
        assert_eq!(url(&search_with(&settings, "!gh").remove(0)), "https://github.com");
    }

    #[test]
    fn unknown_bang_is_searched_literally() {
        assert!(url(&search("!notarealbang42 hi")).starts_with("https://www.startpage.com/"));
    }
}
