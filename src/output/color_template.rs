//! Expand Ruby `NA::Color.template`-style brace sequences into ANSI prefixes (no trailing reset).

use regex::Regex;
use std::sync::OnceLock;

fn hex_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:(f|fg|b|bg))?#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$").expect("hex color regex")
    })
}

fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim();
    if h.len() == 6 {
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        return Some((r, g, b));
    }
    if h.len() == 3 {
        let r = expand_hex_digit(h.chars().next()?)?;
        let g = expand_hex_digit(h.chars().nth(1)?)?;
        let b = expand_hex_digit(h.chars().nth(2)?)?;
        return Some((r, g, b));
    }
    None
}

fn expand_hex_digit(c: char) -> Option<u8> {
    let v = c.to_digit(16)? as u8;
    Some(v * 17)
}

fn try_expand_hex(inner: &str) -> Option<String> {
    let inner = inner.trim();
    let caps = hex_regex().captures(inner)?;
    let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let hex = caps.get(2)?.as_str();
    let (r, g, b) = parse_hex_rgb(hex)?;
    let is_bg = matches!(prefix, "b" | "bg");
    let code = if is_bg { 48 } else { 38 };
    Some(format!("\x1b[{code};2;{r};{g};{b}m"))
}

/// Single-letter / uppercase codes aligned with Ruby `NA::Color` `colors_hash`.
fn ansi_for_style_char(c: char) -> Option<&'static str> {
    Some(match c {
        'w' => "\x1b[37m",
        'k' => "\x1b[30m",
        'g' => "\x1b[32m",
        'l' => "\x1b[34m",
        'y' => "\x1b[33m",
        'c' => "\x1b[36m",
        'm' => "\x1b[35m",
        'r' => "\x1b[31m",
        'W' => "\x1b[47m",
        'K' => "\x1b[40m",
        'G' => "\x1b[42m",
        'L' => "\x1b[44m",
        'Y' => "\x1b[43m",
        'C' => "\x1b[46m",
        'M' => "\x1b[45m",
        'R' => "\x1b[41m",
        'd' => "\x1b[2m",
        'b' => "\x1b[1m",
        'u' => "\x1b[4m",
        'i' => "\x1b[3m",
        'x' => "\x1b[0m",
        _ => return None,
    })
}

fn expand_style_group(inner: &str) -> String {
    let mut out = String::new();
    for c in inner.chars() {
        if let Some(seq) = ansi_for_style_char(c) {
            out.push_str(seq);
        }
    }
    out
}

/// Turns `theme.yaml` color strings into an ANSI prefix (open sequences only).
pub fn expand_color_template(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len() * 2);
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '{' {
            out.push('{');
            i += 2;
            continue;
        }
        if chars[i] != '{' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < chars.len() && chars[j] != '}' {
            j += 1;
        }
        if j >= chars.len() {
            out.push('{');
            i += 1;
            continue;
        }
        let inner: String = chars[start..j].iter().collect();
        i = j + 1;
        if let Some(hex_out) = try_expand_hex(&inner) {
            out.push_str(&hex_out);
        } else {
            out.push_str(&expand_style_group(&inner));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_hex_fg() {
        let s = expand_color_template("{#eccc87}");
        assert_eq!(s, "\x1b[38;2;236;204;135m");
    }

    #[test]
    fn expands_hex_bg() {
        let s = expand_color_template("{b#eccc87}");
        assert_eq!(s, "\x1b[48;2;236;204;135m");
    }

    #[test]
    fn expands_style_chars() {
        let s = expand_color_template("{bc}");
        assert_eq!(s, "\x1b[1m\x1b[36m");
    }

    #[test]
    fn escaped_brace() {
        let s = expand_color_template(r"\{notcode}");
        assert_eq!(s, "{notcode}");
    }
}
