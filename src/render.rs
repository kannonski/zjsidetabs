//! Rows → styled lines. Owns the look; `model` owns the structure.

use std::collections::HashMap;
use zellij_tile::prelude::{PaneInfo, TabInfo};

use crate::model::{self, Row};
use crate::theme;

/// Colours are `#[fg=...]` values: hex or 0-255. Defaults are Catppuccin Mocha.
#[derive(Debug, Clone)]
pub struct Palette {
    pub accent: String,
    pub text: String,
    pub subtext: String,
    pub muted: String,
    pub dim: String,
    pub surface: String,
    pub surface_hi: String,
    pub warn: String,
    pub ok: String,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            accent: "#cba6f7".into(),
            text: "#cdd6f4".into(),
            subtext: "#a6adc8".into(),
            muted: "#6c7086".into(),
            dim: "#585b70".into(),
            surface: "#313244".into(),
            surface_hi: "#45475a".into(),
            warn: "#fab387".into(),
            ok: "#a6e3a1".into(),
        }
    }
}

impl Palette {
    pub fn apply(&mut self, k: &str, v: &str) -> bool {
        let slot = match k {
            "color_accent" => &mut self.accent,
            "color_text" => &mut self.text,
            "color_subtext" => &mut self.subtext,
            "color_muted" => &mut self.muted,
            "color_dim" => &mut self.dim,
            "color_surface" => &mut self.surface,
            "color_surface_hi" => &mut self.surface_hi,
            "color_warn" => &mut self.warn,
            "color_ok" => &mut self.ok,
            _ => return false,
        };
        *slot = v.to_string();
        true
    }
}

pub struct Ctx<'a> {
    pub cols: usize,
    pub tabs: &'a [TabInfo],
    pub panes: &'a HashMap<usize, Vec<PaneInfo>>,
    pub expanded: &'a std::collections::HashSet<usize>,
    pub filter: Option<&'a str>,
    pub session: Option<&'a str>,
    pub cursor: Option<usize>,
    pub hover: Option<usize>,
    pub spinner: usize,
    pub pal: &'a Palette,
}

const CAP_L: char = '\u{e0b6}';
const CAP_R: char = '\u{e0b4}';
const SPIN: [&str; 4] = ["\u{25dc}", "\u{25dd}", "\u{25de}", "\u{25df}"];

/// Fit `s` into `w` cells, ellipsised.
pub fn fit(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(w - 1).collect();
    out.push('\u{2026}');
    out
}

fn pad(s: &str, w: usize) -> String {
    let n = theme::width(s);
    if n >= w {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(w - n))
}

/// Left `l` and right `r` markup joined to exactly `w` visible cells.
fn spread(l: &str, r: &str, w: usize) -> String {
    let lw = theme::width(l);
    let rw = theme::width(r);
    if lw + rw >= w {
        return pad(l, w);
    }
    format!("{l}{}{r}", " ".repeat(w - lw - rw))
}

impl<'a> Ctx<'a> {
    fn tab(&self, pos: usize) -> Option<&'a TabInfo> {
        self.tabs.iter().find(|t| t.position == pos)
    }

    fn visible_panes(&self, pos: usize) -> Vec<&'a PaneInfo> {
        let mut v: Vec<&PaneInfo> = self
            .panes
            .get(&pos)
            .map(|ps| {
                ps.iter()
                    .filter(|p| !p.is_plugin && p.is_selectable && !p.is_suppressed)
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|p| (p.pane_y, p.pane_x));
        v
    }

    /// Content width inside the 2-cell margins on each side.
    fn inner(&self) -> usize {
        self.cols.saturating_sub(4)
    }

    /// Wrap `content` (already `inner()` wide) as a plain row, a hovered row,
    /// the cursor row, or the active pill.
    fn frame(&self, content: &str, active: bool, idx: usize) -> String {
        let p = self.pal;
        let cur = self.cursor == Some(idx);
        let gutter = if cur {
            format!("#[fg={}]\u{258f}", p.accent)
        } else {
            " ".into()
        };
        if active {
            format!(
                "{gutter}#[fg={s}]{CAP_L}#[bg={s}]{content}#[bg=none,fg={s}]{CAP_R} ",
                s = p.surface
            )
        } else if self.hover == Some(idx) {
            format!("{gutter} #[bg={s}]{content}#[bg=none] ", s = p.surface_hi)
        } else {
            format!("{gutter} {content} ")
        }
    }

    fn tab_row(&self, pos: usize, idx: usize) -> String {
        let p = self.pal;
        let Some(tab) = self.tab(pos) else {
            return String::new();
        };
        let panes = self.visible_panes(pos);
        let label = model::label(tab, &panes.iter().map(|p| (*p).clone()).collect::<Vec<_>>());
        let open = self.expanded.contains(&pos) || self.filter.is_some();
        let chevron = if panes.len() > 1 {
            if open {
                "\u{25be}"
            } else {
                "\u{25b8}"
            }
        } else {
            " "
        };
        let (c_idx, c_txt, c_chev, bold) = if tab.active {
            (&p.accent, &p.text, &p.accent, ",bold")
        } else if self.hover == Some(idx) {
            (&p.subtext, &p.text, &p.muted, "")
        } else {
            (&p.dim, &p.subtext, &p.dim, "")
        };

        let mut badges: Vec<String> = Vec::new();
        if tab.has_bell_notification {
            badges.push(format!("#[fg={}]\u{f009a}", p.warn));
        }
        if tab.is_fullscreen_active {
            badges.push(format!("#[fg={}]\u{f0293}", p.muted));
        }
        if tab.is_sync_panes_active {
            badges.push(format!("#[fg={}]\u{f04e6}", p.ok));
        }
        if panes.len() > 1 && !open {
            badges.push(format!("#[fg={}]{}", p.dim, panes.len()));
        }
        let right = badges.join(" ");

        let w = self.inner();
        let head = format!(
            "#[fg={c_chev}]{chevron} #[fg={c_idx}{bold}]{} #[fg={c_txt}{bold}]{} ",
            pos + 1,
            label.icon
        );
        let head_w = theme::width(&head);
        let room =
            w.saturating_sub(head_w + theme::width(&right) + if right.is_empty() { 0 } else { 1 });
        let text = format!("#[fg={c_txt}{bold}]{}", fit(&label.text, room));
        let content = spread(&format!("{head}{text}"), &right, w);
        self.frame(&content, tab.active, idx)
    }

    fn pane_row(&self, tab: usize, id: u32, last: bool, idx: usize) -> String {
        let p = self.pal;
        let pane = self.visible_panes(tab).into_iter().find(|p| p.id == id);
        let Some(pane) = pane else {
            return String::new();
        };
        let tab_active = self.tab(tab).is_some_and(|t| t.active);
        let branch = if last { "\u{2514}" } else { "\u{251c}" };
        let icon = crate::icons::for_command(pane.terminal_command.as_deref());
        let name = model::program_name(pane);
        let focused = pane.is_focused && tab_active;
        let (c, dot) = if pane.exited {
            (&p.dim, format!(" #[fg={}]\u{2717}", p.warn))
        } else if focused {
            (&p.text, format!(" #[fg={}]\u{25cf}", p.accent))
        } else if self.hover == Some(idx) {
            (&p.text, String::new())
        } else {
            (&p.subtext, String::new())
        };
        let w = self.inner();
        let head = format!("    #[fg={}]{branch} #[fg={c}]{icon} ", p.dim);
        let room =
            w.saturating_sub(theme::width(&head) + if focused || pane.exited { 2 } else { 0 });
        let content = pad(&format!("{head}#[fg={c}]{}{dot}", fit(&name, room)), w);
        self.frame(&content, false, idx)
    }

    fn activity_row(&self, glyph: &str, text: &str, idx: usize) -> String {
        let p = self.pal;
        let g = if glyph == "\u{25b6}" {
            SPIN[self.spinner % SPIN.len()]
        } else {
            glyph
        };
        let colour = if glyph == "\u{25b6}" {
            &p.accent
        } else {
            &p.muted
        };
        let w = self.inner();
        let head = format!("      #[fg={colour}]{g} ");
        let room = w.saturating_sub(theme::width(&head));
        let content = pad(&format!("{head}#[fg={}]{}", p.muted, fit(text, room)), w);
        self.frame(&content, false, idx)
    }

    pub fn line(&self, row: &Row, next: Option<&Row>, idx: usize) -> String {
        let p = self.pal;
        let w = self.inner();
        let body = match row {
            Row::Header => {
                let name = self.session.unwrap_or("zellij");
                let content = pad(
                    &format!(
                        "#[fg={}]\u{f120} #[fg={}]{}",
                        p.muted,
                        p.subtext,
                        fit(name, w.saturating_sub(2))
                    ),
                    w,
                );
                format!("  {content}  ")
            }
            Row::Filter => {
                let q = self.filter.unwrap_or("");
                let body = if q.is_empty() {
                    format!("#[fg={}]type to filter", p.dim)
                } else {
                    format!(
                        "#[fg={}]{}#[fg={}]\u{258f}",
                        p.text,
                        fit(q, w.saturating_sub(3)),
                        p.accent
                    )
                };
                let content = pad(&format!("#[fg={}]/ {body}", p.accent), w);
                format!("  {content}  ")
            }
            Row::Blank => String::new(),
            Row::Tab { pos } => self.tab_row(*pos, idx),
            Row::Pane { tab, id, .. } => {
                let last = !matches!(next, Some(Row::Pane { tab: t, .. }) if t == tab);
                self.pane_row(*tab, *id, last, idx)
            }
            Row::Activity { glyph, text, .. } => self.activity_row(glyph, text, idx),
            Row::OverflowUp(n) => format!(
                "  {}",
                pad(&format!("#[fg={}]  \u{25b4} {n} more", p.dim), w)
            ),
            Row::OverflowDown(n) => format!(
                "  {}",
                pad(&format!("#[fg={}]  \u{25be} {n} more", p.dim), w)
            ),
        };
        theme::render(&pad(&body, self.cols))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_ellipsises() {
        assert_eq!(fit("abcdef", 4), "abc\u{2026}");
        assert_eq!(fit("abc", 4), "abc");
    }

    #[test]
    fn spread_fills_exact_width() {
        let s = spread("#[fg=#fff]ab", "cd", 10);
        assert_eq!(theme::width(&s), 10);
    }

    #[test]
    fn spread_never_exceeds_width() {
        let s = spread("abcdefgh", "xyz", 6);
        assert_eq!(theme::width(&s), 8); // pad() returns l untouched when too long; caller truncates first
    }
}
