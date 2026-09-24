//! Pure state → row projection. No zellij calls here, so it is unit-testable.

use std::collections::{BTreeMap, HashMap, HashSet};
use zellij_tile::prelude::{PaneInfo, TabInfo};

use crate::icons;

/// One screen line of the sidebar and what clicking it does.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Header,
    Filter,
    Blank,
    Tab {
        pos: usize,
    },
    Pane {
        tab: usize,
        id: u32,
        is_plugin: bool,
    },
    Activity {
        tab: usize,
        text: String,
        glyph: String,
    },
    OverflowUp(usize),
    OverflowDown(usize),
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Activity {
    #[serde(default)]
    pub zsession: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub subagents: BTreeMap<String, Subagent>,
    #[serde(default)]
    pub todos: Vec<Todo>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Subagent {
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub glyph: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Todo {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub text: String,
}

impl Activity {
    /// Rows to draw under the tab: sub-agents win over todos, done todos are
    /// hidden, at most 6 lines — matches the producer contract of
    /// cfal/zellij-vertical-tabs so existing Claude Code hooks keep working.
    pub fn lines(&self) -> Vec<(String, String)> {
        if !self.subagents.is_empty() {
            return self
                .subagents
                .values()
                .map(|s| {
                    let icon = if s.icon.is_empty() {
                        "\u{229c}"
                    } else {
                        &s.icon
                    };
                    let glyph = if s.glyph.is_empty() {
                        "\u{2699}"
                    } else {
                        &s.glyph
                    };
                    (format!("{icon} {glyph}"), s.title.clone())
                })
                .collect();
        }
        let mut out: Vec<(String, String)> = self
            .todos
            .iter()
            .filter(|t| t.status != "done" && t.status != "completed")
            .map(|t| {
                let g = if t.status == "in_progress" {
                    "\u{25b6}"
                } else {
                    "\u{25cb}"
                };
                (g.to_string(), t.text.clone())
            })
            .take(6)
            .collect();
        let hidden = self
            .todos
            .iter()
            .filter(|t| t.status != "done" && t.status != "completed")
            .count();
        if hidden > 6 {
            out.push(("\u{2026}".into(), format!("{} more", hidden - 6)));
        }
        out
    }
}

/// A tab's derived label: what a human calls this tab.
pub struct Label {
    pub icon: &'static str,
    pub text: String,
    /// True when the text came from a pane rather than a user rename, i.e.
    /// it is safe for auto-rename to overwrite.
    pub derived: bool,
}

/// Names that can only have come from zellij or from this plugin's own
/// derivation — never from a person — and may therefore be overwritten.
/// `exit`/`logout` are what a shell titles itself in its last moment; a tab
/// carrying one was named by an earlier build and is stale.
pub fn is_default_name(name: &str) -> bool {
    name.starts_with("Tab #") || matches!(name, "exit" | "logout" | "zsh" | "bash" | "fish" | "sh")
}

/// Label for a tab. Preference: user rename > focused pane's command >
/// focused pane's title > zellij default.
pub fn label(tab: &TabInfo, panes: &[PaneInfo]) -> Label {
    let live = |p: &&PaneInfo| !p.is_plugin && p.is_selectable && !p.exited;
    let focused = panes
        .iter()
        .filter(live)
        .find(|p| p.is_focused)
        .or_else(|| panes.iter().find(live));
    let icon = icons::for_command(focused.and_then(|p| p.terminal_command.as_deref()));
    if !is_default_name(&tab.name) {
        return Label {
            icon,
            text: tab.name.clone(),
            derived: false,
        };
    }
    match focused.and_then(derived_label) {
        Some(text) => Label {
            icon,
            text,
            derived: true,
        },
        None => Label {
            icon,
            text: tab.name.clone(),
            derived: false,
        },
    }
}

/// A name stable enough to write onto a tab: the command's basename, or for
/// a bare shell the cwd out of a `user@host:path` title. Anything else — a
/// transient title such as `exit`, a program's free-form title — yields
/// None: display it, but never rename after it.
pub fn derived_label(p: &PaneInfo) -> Option<String> {
    if let Some(cmd) = p.terminal_command.as_deref() {
        let mut parts = cmd.split_whitespace().filter(|s| !s.contains('='));
        if let Some(first) = parts.next() {
            let base = first.rsplit('/').next().unwrap_or(first);
            if matches!(base, "sudo" | "env" | "npx" | "uvx" | "bunx") {
                if let Some(n) = parts.next() {
                    return Some(n.rsplit('/').next().unwrap_or(n).to_string());
                }
            }
            return Some(base.to_string());
        }
    }
    shell_label(&p.title)
}

/// Display name for a pane row: the derived label, else the raw title.
pub fn program_name(p: &PaneInfo) -> String {
    derived_label(p).unwrap_or_else(|| p.title.clone())
}

/// `okkan@host:~/Project/gitlab` → `gitlab`; `~` → `~`; `/` → `/`.
/// None when the title is not in `user@host:path` form.
pub fn shell_label(title: &str) -> Option<String> {
    let (who, path) = title.split_once(':')?;
    if !who.contains('@') || path.contains(' ') || path.is_empty() {
        return None;
    }
    if path == "/" || path == "~" {
        return Some(path.to_string());
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return Some("/".to_string());
    }
    Some(trimmed.rsplit('/').next().unwrap_or(trimmed).to_string())
}

pub struct Projection<'a> {
    pub tabs: &'a [TabInfo],
    pub panes: &'a HashMap<usize, Vec<PaneInfo>>,
    pub expanded: &'a HashSet<usize>,
    pub filter: Option<&'a str>,
    pub activity: &'a HashMap<String, Activity>,
    pub session: Option<&'a str>,
    pub show_header: bool,
    pub show_tree: bool,
}

impl<'a> Projection<'a> {
    fn tab_panes(&self, pos: usize) -> Vec<&'a PaneInfo> {
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

    fn matches(&self, tab: &TabInfo, panes: &[&PaneInfo]) -> bool {
        let Some(q) = self.filter else { return true };
        let q = q.to_lowercase();
        if q.is_empty() {
            return true;
        }
        tab.name.to_lowercase().contains(&q)
            || panes.iter().any(|p| {
                p.title.to_lowercase().contains(&q)
                    || p.terminal_command
                        .as_deref()
                        .is_some_and(|c| c.to_lowercase().contains(&q))
            })
    }

    fn activity_for(&self, tab: &TabInfo, panes: &[&PaneInfo]) -> Option<&'a Activity> {
        let session = self.session.unwrap_or("");
        self.activity.values().find(|a| {
            (a.zsession.is_empty() || a.zsession == session)
                && (a.name == tab.name
                    || panes
                        .iter()
                        .any(|p| p.title == a.name || p.is_focused && program_name(p) == a.name))
        })
    }

    /// The full, unscrolled row list.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.show_header {
            rows.push(Row::Header);
        }
        if self.filter.is_some() {
            rows.push(Row::Filter);
        }
        if self.show_header || self.filter.is_some() {
            rows.push(Row::Blank);
        }
        for tab in self.tabs {
            let panes = self.tab_panes(tab.position);
            if !self.matches(tab, &panes) {
                continue;
            }
            rows.push(Row::Tab { pos: tab.position });
            let open = self.expanded.contains(&tab.position) || self.filter.is_some();
            if self.show_tree && open && panes.len() > 1 {
                for p in &panes {
                    rows.push(Row::Pane {
                        tab: tab.position,
                        id: p.id,
                        is_plugin: p.is_plugin,
                    });
                }
            }
            if let Some(act) = self.activity_for(tab, &panes) {
                for (glyph, text) in act.lines() {
                    rows.push(Row::Activity {
                        tab: tab.position,
                        glyph,
                        text,
                    });
                }
            }
        }
        rows
    }
}

/// Window `rows` onto `height` lines, keeping `keep` (the active tab's row
/// index) visible, with overflow markers that replace the first/last line.
pub fn viewport(
    rows: Vec<Row>,
    height: usize,
    scroll: usize,
    keep: Option<usize>,
) -> (Vec<Row>, usize) {
    if height == 0 {
        return (Vec::new(), 0);
    }
    if rows.len() <= height {
        return (rows, 0);
    }
    let mut scroll = scroll.min(rows.len().saturating_sub(height));
    if let Some(k) = keep {
        // Reserve one line top and bottom for markers when scrolled.
        let inner = height.saturating_sub(2).max(1);
        if k < scroll + 1 {
            scroll = k.saturating_sub(1);
        } else if k >= scroll + 1 + inner {
            scroll = k + 2 - height;
        }
        scroll = scroll.min(rows.len().saturating_sub(height));
    }
    let end = (scroll + height).min(rows.len());
    let mut out: Vec<Row> = rows[scroll..end].to_vec();
    if scroll > 0 {
        out[0] = Row::OverflowUp(scroll);
    }
    if end < rows.len() {
        let last = out.len() - 1;
        out[last] = Row::OverflowDown(rows.len() - end);
    }
    (out, scroll)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(pos: usize, name: &str, active: bool) -> TabInfo {
        TabInfo {
            position: pos,
            name: name.into(),
            active,
            ..Default::default()
        }
    }
    fn pane(id: u32, cmd: &str, focused: bool) -> PaneInfo {
        PaneInfo {
            id,
            is_focused: focused,
            is_selectable: true,
            terminal_command: Some(cmd.into()),
            title: cmd.into(),
            ..Default::default()
        }
    }
    fn proj<'a>(
        tabs: &'a [TabInfo],
        panes: &'a HashMap<usize, Vec<PaneInfo>>,
        expanded: &'a HashSet<usize>,
        filter: Option<&'a str>,
        act: &'a HashMap<String, Activity>,
    ) -> Projection<'a> {
        Projection {
            tabs,
            panes,
            expanded,
            filter,
            activity: act,
            session: Some("main"),
            show_header: false,
            show_tree: true,
        }
    }

    #[test]
    fn folded_tab_hides_panes_expanded_shows_them() {
        let tabs = vec![tab(0, "Tab #1", true)];
        let mut panes = HashMap::new();
        panes.insert(0, vec![pane(1, "nvim", true), pane(2, "zsh", false)]);
        let act = HashMap::new();
        let none = HashSet::new();
        assert_eq!(proj(&tabs, &panes, &none, None, &act).rows().len(), 1);
        let open: HashSet<usize> = [0].into();
        assert_eq!(proj(&tabs, &panes, &open, None, &act).rows().len(), 3);
    }

    #[test]
    fn single_pane_tab_never_shows_a_tree() {
        let tabs = vec![tab(0, "Tab #1", true)];
        let mut panes = HashMap::new();
        panes.insert(0, vec![pane(1, "nvim", true)]);
        let open: HashSet<usize> = [0].into();
        assert_eq!(
            proj(&tabs, &panes, &open, None, &HashMap::new())
                .rows()
                .len(),
            1
        );
    }

    #[test]
    fn filter_matches_pane_command_and_expands() {
        let tabs = vec![tab(0, "Tab #1", true), tab(1, "Tab #2", false)];
        let mut panes = HashMap::new();
        panes.insert(0, vec![pane(1, "nvim", true), pane(2, "zsh", false)]);
        panes.insert(1, vec![pane(3, "docker ps", true)]);
        let rows = proj(
            &tabs,
            &panes,
            &HashSet::new(),
            Some("dock"),
            &HashMap::new(),
        )
        .rows();
        // Filter row + blank + the one matching tab.
        assert!(matches!(rows[0], Row::Filter));
        assert_eq!(
            rows.iter().filter(|r| matches!(r, Row::Tab { .. })).count(),
            1
        );
        assert!(rows.contains(&Row::Tab { pos: 1 }));
    }

    #[test]
    fn label_prefers_user_rename_then_command() {
        let p = vec![pane(1, "/usr/bin/nvim x.rs", true)];
        let l = label(&tab(0, "Tab #1", true), &p);
        assert_eq!(l.text, "nvim");
        assert!(l.derived);
        let l = label(&tab(0, "infra", true), &p);
        assert_eq!(l.text, "infra");
        assert!(!l.derived);
    }

    #[test]
    fn shell_title_collapses_to_cwd_basename() {
        assert_eq!(
            shell_label("okkan@okkan:~/Project/gitlab").as_deref(),
            Some("gitlab")
        );
        assert_eq!(shell_label("okkan@okkan:~").as_deref(), Some("~"));
        assert_eq!(shell_label("okkan@okkan:/").as_deref(), Some("/"));
        assert_eq!(shell_label("nvim main.rs"), None);
        assert_eq!(shell_label("exit"), None);
        assert_eq!(shell_label("a@b: has space"), None);
    }

    #[test]
    fn transient_title_never_becomes_a_tab_name() {
        let mut p = pane(1, "zsh", true);
        p.terminal_command = None;
        p.title = "exit".into();
        let l = label(&tab(0, "Tab #1", true), &[p.clone()]);
        assert!(
            !l.derived,
            "a bare `exit` title must not be written to the tab"
        );
        p.exited = true;
        p.title = "okkan@h:~/x".into();
        let l = label(&tab(0, "Tab #1", true), &[p]);
        assert!(!l.derived, "an exited pane must not name the tab");
    }

    #[test]
    fn stale_exit_name_is_reclaimable() {
        assert!(is_default_name("exit"));
        assert!(is_default_name("Tab #4"));
        assert!(!is_default_name("infra"));
    }

    #[test]
    fn activity_attaches_by_tab_name_and_hides_done() {
        let tabs = vec![tab(0, "build", true)];
        let panes = HashMap::new();
        let mut act = HashMap::new();
        act.insert(
            "k".into(),
            Activity {
                zsession: "main".into(),
                name: "build".into(),
                todos: vec![
                    Todo {
                        status: "done".into(),
                        text: "a".into(),
                    },
                    Todo {
                        status: "in_progress".into(),
                        text: "b".into(),
                    },
                ],
                ..Default::default()
            },
        );
        let rows = proj(&tabs, &panes, &HashSet::new(), None, &act).rows();
        assert_eq!(rows.len(), 2);
        assert!(matches!(&rows[1], Row::Activity { text, .. } if text == "b"));
    }

    #[test]
    fn viewport_keeps_active_visible_with_markers() {
        let rows: Vec<Row> = (0..20).map(|i| Row::Tab { pos: i }).collect();
        let (v, scroll) = viewport(rows, 6, 0, Some(15));
        assert_eq!(v.len(), 6);
        assert!(matches!(v[0], Row::OverflowUp(_)));
        assert!(v.contains(&Row::Tab { pos: 15 }));
        assert!(scroll > 0);
    }

    #[test]
    fn viewport_fits_without_markers() {
        let rows: Vec<Row> = (0..3).map(|i| Row::Tab { pos: i }).collect();
        let (v, _) = viewport(rows.clone(), 10, 0, Some(1));
        assert_eq!(v, rows);
    }
}
