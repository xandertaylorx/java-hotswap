//! Java .properties syntax, including escaped separators and continued lines.
use std::{collections::BTreeMap, fs, path::Path};

fn unescape(s: &str) -> Result<String, String> {
    let mut chars = s.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('f') => out.push('\x0c'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if hex.len() != 4 {
                    return Err("Incomplete Unicode property escape".into());
                }
                let code =
                    u32::from_str_radix(&hex, 16).map_err(|_| "Invalid Unicode property escape")?;
                out.push(char::from_u32(code).ok_or("Invalid Unicode property character")?);
            }
            Some(c) => out.push(c),
            None => out.push('\\'),
        }
    }
    Ok(out)
}
pub fn parse(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    let mut pending = String::new();
    for line in text.lines() {
        let line = line.trim_start_matches([' ', '\t', '\x0c']);
        if pending.is_empty() && (line.starts_with(['#', '!']) || line.is_empty()) {
            continue;
        }
        pending.push_str(line);
        if pending.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1 {
            pending.pop();
            continue;
        }
        let mut escaped = false;
        let mut split = pending.len();
        for (i, c) in pending.char_indices() {
            if !escaped && matches!(c, '=' | ':' | ' ' | '\t' | '\x0c') {
                split = i;
                break;
            }
            escaped = c == '\\' && !escaped;
        }
        let mut value = pending[split..].trim_start_matches([' ', '\t', '\x0c']);
        if value.starts_with(['=', ':']) {
            value = value[1..].trim_start_matches([' ', '\t', '\x0c']);
        }
        result.insert(unescape(&pending[..split])?, unescape(value)?);
        pending.clear();
    }
    if !pending.is_empty() {
        return Err("Unterminated property continuation".into());
    }
    Ok(result)
}
pub fn read(path: &Path) -> Result<BTreeMap<String, String>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    parse(&fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?)
        .map_err(|e| format!("Invalid {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapper_urls_and_windows_paths() {
        let p = parse("# comment\ndistributionUrl=https\\://mirror/\\\n  gradle-8.10-bin.zip\norg.gradle.java.home C\\:\\\\Program Files\\\\Java\\\\jdk-21\nkey\\:name = value\\u0020ok\n").unwrap();
        assert_eq!(p["distributionUrl"], "https://mirror/gradle-8.10-bin.zip");
        assert_eq!(p["org.gradle.java.home"], "C:\\Program Files\\Java\\jdk-21");
        assert_eq!(p["key:name"], "value ok");
    }
}
