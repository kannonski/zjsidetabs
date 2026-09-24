//! Inline `#[fg=...,bg=...,bold]` markup, rendered to ANSI.

pub struct Style {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub dim: bool,
}

/// Render `#[...]`-annotated text to an ANSI string. Unknown keys are dropped
/// rather than printed, so a typo degrades to plain text instead of leaking
/// markup into the sidebar.
pub fn render(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("#[") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find(']') else {
            out.push_str(&rest[start..]);
            return out;
        };
        out.push_str(&ansi(&parse(&after[..end])));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out.push_str("\u{1b}[0m");
    out
}

fn parse(spec: &str) -> Style {
    let mut s = Style {
        fg: None,
        bg: None,
        bold: false,
        dim: false,
    };
    for part in spec.split(',') {
        let part = part.trim();
        match part {
            "bold" => s.bold = true,
            "dim" | "dimmed" => s.dim = true,
            _ => match part.split_once('=') {
                Some(("fg", v)) => s.fg = colour(v),
                Some(("bg", v)) => s.bg = colour(v),
                _ => {}
            },
        }
    }
    s
}

/// `#rrggbb`, `#rgb`, a 0-255 palette index, or `none`.
fn colour(v: &str) -> Option<String> {
    let v = v.trim();
    if v == "none" || v == "default" || v == "reset" {
        return None;
    }
    if let Some(hex) = v.strip_prefix('#') {
        let expand = |c: u8| {
            let d = (c as char).to_digit(16).unwrap_or(0) as u8;
            d * 17
        };
        return match hex.len() {
            6 => u32::from_str_radix(hex, 16)
                .ok()
                .map(|n| format!("2;{};{};{}", (n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff)),
            3 => {
                let b = hex.as_bytes();
                Some(format!(
                    "2;{};{};{}",
                    expand(b[0]),
                    expand(b[1]),
                    expand(b[2])
                ))
            }
            _ => None,
        };
    }
    v.parse::<u8>().ok().map(|n| format!("5;{n}"))
}

fn ansi(s: &Style) -> String {
    let mut out = String::from("\u{1b}[0m");
    if s.bold {
        out.push_str("\u{1b}[1m");
    }
    if s.dim {
        out.push_str("\u{1b}[2m");
    }
    if let Some(fg) = &s.fg {
        out.push_str(&format!("\u{1b}[38;{fg}m"));
    }
    if let Some(bg) = &s.bg {
        out.push_str(&format!("\u{1b}[48;{bg}m"));
    }
    out
}

/// Visible width, ignoring markup and ANSI. Used for click hit-testing and
/// truncation, so it must not count escape bytes.
pub fn width(input: &str) -> usize {
    let mut n = 0;
    let mut rest = input;
    while let Some(start) = rest.find("#[") {
        n += rest[..start].chars().count();
        let after = &rest[start + 2..];
        match after.find(']') {
            Some(end) => rest = &after[end + 1..],
            None => return n + after.chars().count(),
        }
    }
    n + rest.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_fg_becomes_truecolour() {
        assert!(render("#[fg=#cba6f7]x").contains("38;2;203;166;247"));
    }

    #[test]
    fn short_hex_expands() {
        assert!(render("#[fg=#fff]x").contains("38;2;255;255;255"));
    }

    #[test]
    fn palette_index_uses_256_form() {
        assert!(render("#[fg=183]x").contains("38;5;183"));
    }

    #[test]
    fn unknown_key_is_dropped_not_printed() {
        let out = render("#[wat=1]hello");
        assert!(out.contains("hello") && !out.contains("wat"));
    }

    #[test]
    fn unterminated_markup_is_passed_through() {
        assert!(render("#[fg=#fff hello").contains("#[fg=#fff hello"));
    }

    #[test]
    fn width_ignores_markup() {
        assert_eq!(width("#[fg=#fff,bold]abc#[bg=none]de"), 5);
    }
}
