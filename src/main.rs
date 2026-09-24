//! zjsidetabs — a vertical, foldable tab rail for zellij.

mod icons;
mod model;
mod render;
mod theme;

use std::collections::{BTreeMap, HashMap, HashSet};

use zellij_tile::prelude::*;

use model::{Activity, Projection, Row};
use render::{Ctx, Palette};

const SPINNER_SECS: f64 = 0.2;

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    panes: HashMap<usize, Vec<PaneInfo>>,
    session: Option<String>,
    /// Tabs the user unfolded. The active tab is unfolded implicitly when
    /// `auto_expand` is on, so it is never stored here.
    expanded: HashSet<usize>,
    /// Tabs the user explicitly folded while active, overriding `auto_expand`.
    folded: HashSet<usize>,
    filter: Option<String>,
    activity: HashMap<String, Activity>,
    /// Names this plugin wrote via auto-rename, so it knows which tab names
    /// it may overwrite and which are the user's.
    our_names: HashMap<usize, String>,
    rows: Vec<Row>,
    scroll: usize,
    cursor: Option<usize>,
    hover: Option<usize>,
    spinner: usize,
    timer_armed: bool,
    cols: usize,
    height: usize,
    pal: Palette,
    auto_expand: bool,
    auto_rename: bool,
    show_header: bool,
    show_tree: bool,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, cfg: BTreeMap<String, String>) {
        self.auto_expand = true;
        self.auto_rename = false;
        self.show_header = true;
        self.show_tree = true;
        for (k, v) in &cfg {
            if self.pal.apply(k, v) {
                continue;
            }
            let on = matches!(v.as_str(), "true" | "1" | "yes" | "on");
            match k.as_str() {
                "auto_expand" => self.auto_expand = on,
                "auto_rename" => self.auto_rename = on,
                "show_header" => self.show_header = on,
                "show_tree" => self.show_tree = on,
                _ => {}
            }
        }
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Key,
            EventType::Mouse,
            EventType::Timer,
            EventType::PermissionRequestResult,
        ]);
        set_selectable(true);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(_) => true,
            Event::ModeUpdate(m) => {
                self.session = m.session_name;
                true
            }
            Event::TabUpdate(tabs) => {
                self.tabs = tabs;
                self.tabs.sort_by_key(|t| t.position);
                self.maybe_rename();
                true
            }
            Event::PaneUpdate(m) => {
                self.panes = m.panes;
                self.maybe_rename();
                true
            }
            Event::Key(k) => self.key(k),
            Event::Mouse(m) => self.mouse(m),
            Event::Timer(_) => {
                self.timer_armed = false;
                self.spinner = self.spinner.wrapping_add(1);
                true
            }
            _ => false,
        }
    }

    fn pipe(&mut self, msg: PipeMessage) -> bool {
        if msg.name != "activity" {
            return false;
        }
        let Some(payload) = msg.payload else {
            return false;
        };
        match serde_json::from_str::<Activity>(&payload) {
            Ok(a) => {
                let key = format!("{}\u{0}{}", a.zsession, a.name);
                if a.subagents.is_empty() && a.todos.is_empty() {
                    self.activity.remove(&key);
                } else {
                    self.activity.insert(key, a);
                }
                true
            }
            Err(_) => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.cols = cols;
        self.height = rows;
        let expanded = self.effective_expanded();
        let all = Projection {
            tabs: &self.tabs,
            panes: &self.panes,
            expanded: &expanded,
            filter: self.filter.as_deref(),
            activity: &self.activity,
            session: self.session.as_deref(),
            show_header: self.show_header,
            show_tree: self.show_tree,
        }
        .rows();

        let active_idx = self.tabs.iter().find(|t| t.active).and_then(|t| {
            all.iter()
                .position(|r| matches!(r, Row::Tab { pos } if *pos == t.position))
        });
        let (view, scroll) = model::viewport(all, rows, self.scroll, active_idx);
        self.scroll = scroll;
        if let Some(c) = self.cursor {
            if c >= view.len() {
                self.cursor = view.len().checked_sub(1);
            }
        }

        let ctx = Ctx {
            cols,
            tabs: &self.tabs,
            panes: &self.panes,
            expanded: &expanded,
            filter: self.filter.as_deref(),
            session: self.session.as_deref(),
            cursor: self.cursor,
            hover: self.hover,
            spinner: self.spinner,
            pal: &self.pal,
        };
        let mut out = String::new();
        for (i, row) in view.iter().enumerate() {
            out.push_str(&ctx.line(row, view.get(i + 1), i));
            if i + 1 < rows {
                out.push('\n');
            }
        }
        print!("{out}");
        self.rows = view;

        let running = self
            .rows
            .iter()
            .any(|r| matches!(r, Row::Activity { glyph, .. } if glyph == "\u{25b6}"));
        if running && !self.timer_armed {
            self.timer_armed = true;
            set_timeout(SPINNER_SECS);
        }
    }
}

impl State {
    fn effective_expanded(&self) -> HashSet<usize> {
        let mut e = self.expanded.clone();
        if self.auto_expand {
            if let Some(t) = self.tabs.iter().find(|t| t.active) {
                if !self.folded.contains(&t.position) {
                    e.insert(t.position);
                }
            }
        }
        e
    }

    fn toggle(&mut self, pos: usize) {
        let active = self.tabs.iter().any(|t| t.active && t.position == pos);
        let open = self.effective_expanded().contains(&pos);
        if open {
            self.expanded.remove(&pos);
            if active && self.auto_expand {
                self.folded.insert(pos);
            }
        } else {
            self.expanded.insert(pos);
            self.folded.remove(&pos);
        }
    }

    /// Give default-named tabs a name from what runs in them. Only touches
    /// names zellij generated or that this plugin wrote earlier.
    fn maybe_rename(&mut self) {
        if !self.auto_rename {
            return;
        }
        for tab in &self.tabs {
            let ours = self
                .our_names
                .get(&tab.position)
                .is_some_and(|n| *n == tab.name);
            if !model::is_default_name(&tab.name) && !ours {
                continue;
            }
            let panes = self.panes.get(&tab.position).cloned().unwrap_or_default();
            let l = model::label(tab, &panes);
            if l.derived && !model::is_default_name(&l.text) && l.text != tab.name {
                rename_tab_with_id(tab.tab_id as u64, &l.text);
                self.our_names.insert(tab.position, l.text);
            }
        }
    }

    fn activate(&mut self, row: usize) {
        match self.rows.get(row).cloned() {
            Some(Row::Tab { pos }) => switch_tab_to(pos as u32 + 1),
            Some(Row::Pane { id, is_plugin, .. }) => {
                if is_plugin {
                    focus_plugin_pane(id, false, true);
                } else {
                    focus_terminal_pane(id, false, true);
                }
            }
            Some(Row::Activity { tab, .. }) => switch_tab_to(tab as u32 + 1),
            Some(Row::OverflowUp(_)) => {
                self.scroll = self
                    .scroll
                    .saturating_sub(self.height.saturating_sub(2).max(1))
            }
            Some(Row::OverflowDown(_)) => self.scroll += self.height.saturating_sub(2).max(1),
            _ => {}
        }
    }

    fn row_tab(&self, row: usize) -> Option<usize> {
        match self.rows.get(row)? {
            Row::Tab { pos } => Some(*pos),
            Row::Pane { tab, .. } | Row::Activity { tab, .. } => Some(*tab),
            _ => None,
        }
    }

    fn selectable(&self, i: usize) -> bool {
        matches!(
            self.rows.get(i),
            Some(Row::Tab { .. } | Row::Pane { .. } | Row::Activity { .. })
        )
    }

    fn move_cursor(&mut self, down: bool) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let mut i = self.cursor.unwrap_or_else(|| {
            self.tabs
                .iter()
                .find(|t| t.active)
                .and_then(|t| {
                    self.rows
                        .iter()
                        .position(|r| matches!(r, Row::Tab { pos } if *pos == t.position))
                })
                .unwrap_or(0)
        });
        for _ in 0..n {
            i = if down { (i + 1) % n } else { (i + n - 1) % n };
            if self.selectable(i) {
                break;
            }
        }
        self.cursor = Some(i);
    }

    fn key(&mut self, k: KeyWithModifier) -> bool {
        if let Some(q) = self.filter.as_mut() {
            match k.bare_key {
                BareKey::Esc => self.filter = None,
                BareKey::Enter => {
                    let first = self.rows.iter().position(|r| matches!(r, Row::Tab { .. }));
                    self.filter = None;
                    if let Some(i) = first {
                        self.activate(i);
                    }
                }
                BareKey::Backspace => {
                    q.pop();
                }
                BareKey::Char(c) if !k.key_modifiers.contains(&KeyModifier::Ctrl) => q.push(c),
                BareKey::Down => self.move_cursor(true),
                BareKey::Up => self.move_cursor(false),
                _ => return false,
            }
            return true;
        }
        match k.bare_key {
            BareKey::Char('/') => self.filter = Some(String::new()),
            BareKey::Char('j') | BareKey::Down => self.move_cursor(true),
            BareKey::Char('k') | BareKey::Up => self.move_cursor(false),
            BareKey::Char('g') => {
                self.cursor = self.rows.iter().position(|r| matches!(r, Row::Tab { .. }))
            }
            BareKey::Char('G') => {
                self.cursor = self.rows.iter().rposition(|r| matches!(r, Row::Tab { .. }))
            }
            BareKey::Enter | BareKey::Char('l') if self.cursor.is_some() => {
                if let Some(c) = self.cursor {
                    if k.bare_key == BareKey::Char('l') {
                        if let Some(t) = self.row_tab(c) {
                            if !self.effective_expanded().contains(&t) {
                                self.toggle(t);
                                return true;
                            }
                        }
                    }
                    self.activate(c);
                }
            }
            BareKey::Char(' ') | BareKey::Char('h') | BareKey::Tab => {
                if let Some(t) = self.cursor.and_then(|c| self.row_tab(c)) {
                    self.toggle(t);
                }
            }
            BareKey::Char('z') => {
                let all: Vec<usize> = self.tabs.iter().map(|t| t.position).collect();
                if self.effective_expanded().len() > 1 {
                    self.expanded.clear();
                    self.folded.extend(all);
                } else {
                    self.folded.clear();
                    self.expanded.extend(all);
                }
            }
            BareKey::Esc => self.cursor = None,
            _ => return false,
        }
        true
    }

    fn mouse(&mut self, m: Mouse) -> bool {
        match m {
            Mouse::LeftClick(line, col) => {
                let Ok(i) = usize::try_from(line) else {
                    return false;
                };
                let Some(row) = self.rows.get(i).cloned() else {
                    return false;
                };
                // The chevron sits in the first 4 cells of a tab row; clicking
                // it folds instead of switching, like a file tree.
                if let Row::Tab { pos } = row {
                    let multi = self.panes.get(&pos).map_or(0, |p| {
                        p.iter().filter(|p| !p.is_plugin && p.is_selectable).count()
                    }) > 1;
                    if multi && col < 4 {
                        self.toggle(pos);
                        return true;
                    }
                }
                self.activate(i);
                true
            }
            Mouse::RightClick(line, _) => {
                if let Some(t) = usize::try_from(line).ok().and_then(|i| self.row_tab(i)) {
                    self.toggle(t);
                    return true;
                }
                false
            }
            Mouse::Hover(line, _) => {
                let h = usize::try_from(line).ok().filter(|i| self.selectable(*i));
                if h != self.hover {
                    self.hover = h;
                    return true;
                }
                false
            }
            Mouse::ScrollUp(n) => {
                if self
                    .rows
                    .iter()
                    .any(|r| matches!(r, Row::OverflowUp(_) | Row::OverflowDown(_)))
                {
                    self.scroll = self.scroll.saturating_sub(n);
                } else {
                    self.step_tab(false);
                }
                true
            }
            Mouse::ScrollDown(n) => {
                if self
                    .rows
                    .iter()
                    .any(|r| matches!(r, Row::OverflowUp(_) | Row::OverflowDown(_)))
                {
                    self.scroll += n;
                } else {
                    self.step_tab(true);
                }
                true
            }
            _ => false,
        }
    }

    fn step_tab(&self, next: bool) {
        let n = self.tabs.len();
        if n == 0 {
            return;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.active) {
            let j = if next { (i + 1) % n } else { (i + n - 1) % n };
            switch_tab_to(self.tabs[j].position as u32 + 1);
        }
    }
}
