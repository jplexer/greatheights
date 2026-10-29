//! Minimal freedesktop `.desktop` file parsing: just what a launcher needs.

#[derive(Debug, PartialEq)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: Vec<String>,
    pub icon: Option<String>,
}

/// Returns `None` for entries that shouldn't be shown (hidden, not an
/// application, or without a command).
pub fn parse(source: &str) -> Option<DesktopEntry> {
    let mut in_main_group = false;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;

    for line in source.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_main_group || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Type" if value != "Application" => return None,
            "NoDisplay" | "Hidden" if value == "true" => return None,
            "Name" => name = Some(value.to_owned()),
            "Exec" => exec = Some(split_exec(value)),
            "Icon" => icon = Some(value.to_owned()),
            _ => {}
        }
    }

    let exec = exec.filter(|args| !args.is_empty())?;
    Some(DesktopEntry {
        name: name?,
        exec,
        icon,
    })
}

/// Splits an `Exec` value into arguments, honouring double quotes and
/// dropping field codes such as `%U`, since we launch without files.
fn split_exec(value: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_arg = false;
    let mut chars = value.chars();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_arg = true;
            }
            '\\' if in_quotes => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ' ' | '\t' if !in_quotes => {
                if has_arg {
                    args.push(std::mem::take(&mut current));
                    has_arg = false;
                }
            }
            '%' => match chars.next() {
                Some('%') => {
                    current.push('%');
                    has_arg = true;
                }
                // Any other field code expands to nothing.
                _ => {}
            },
            c => {
                current.push(c);
                has_arg = true;
            }
        }
    }
    if has_arg {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_main_group_only() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\nIcon=firefox\n\
             [Desktop Action new-window]\nName=New Window\nExec=firefox --new-window %u\n",
        )
        .unwrap();
        assert_eq!(entry.name, "Firefox");
        assert_eq!(entry.exec, ["firefox"]);
        assert_eq!(entry.icon.as_deref(), Some("firefox"));
    }

    #[test]
    fn skips_hidden_entries() {
        assert!(parse("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true").is_none());
        assert!(parse("[Desktop Entry]\nType=Link\nName=X\nURL=https://x").is_none());
    }

    #[test]
    fn splits_quoted_exec() {
        assert_eq!(
            split_exec(r#""/opt/My App/bin" --flag "a \"b\"" %F 100%%"#),
            ["/opt/My App/bin", "--flag", "a \"b\"", "100%"]
        );
    }
}
