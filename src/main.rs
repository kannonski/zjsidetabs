//! zjsidetabs — a vertical, foldable tab rail for zellij.

mod icons;
mod model;
mod render;
mod theme;

use std::collections::{BTreeMap, HashMap, HashSet};

use zellij_tile::prelude::*;
use zellij_utils::input::layout::PercentOrFixed;

use model::{Activity, Projection, Row};
use render::{Ctx, Palette};

const SPINNER_SECS: f64 = 0.2;
/// How long the mouse must be gone before a hover-expanded rail collapses.
const PEEK_SECS: f64 = 1.0;
/// Widths at or below this render as the minimized band.
const MINI_COLS: usize = 3;
const DEFAULT_FULL_WIDTH: usize = 30;
/// Timer ticks a bell flash lasts (at SPINNER_SECS each).
const FLASH_TICKS: u8 = 6;

/// How this instance is mounted. `Rail` is the single-pane form (tiled or
/// floating). `Handle` + `Dock` are a pair: a thin tiled strip that reserves
/// its columns and shows tab numbers, and a floating full rail that the
/// handle summons on hover and that hides itself when the mouse leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Role {
    #[default]
    Rail,
    Handle,
    Dock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Full,
    Mini,
}

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
    mode: Mode,
    /// Expanded by hover rather than on purpose; collapses when the mouse leaves.
    peek: bool,
    hover_seen: bool,
    hover_expand: bool,
    /// Width to restore to. Follows the user's manual resizes while Full.
    full_width: usize,
    plugin_id: u32,
    /// Cols at the last resize request; seeing them again means zellij
    /// refused (min/max reached) and we stop asking.
    resize_from: Option<usize>,
    /// Set once a terminal pane has been seen next to us in our own tab.
    /// Because this pane is selectable, zellij will not close the tab when
    /// the last shell exits; when that happens we close ourselves so the
    /// tab goes with us, like the built-in bars do.
    had_terminal: bool,
    /// Inline tab rename in progress: (tab position, buffer).
    rename: Option<(usize, String)>,
    /// Mouse-down on a tab row: (row index, tab position). A release on a
    /// different tab row reorders.
    drag: Option<(usize, usize)>,
    /// Tabs whose bell just rang, with ticks of flash left.
    flash: HashMap<usize, u8>,
    bell_seen: HashSet<usize>,
    zellij_bin: String,
    /// Running as a floating pane: geometry is set directly with
    /// change_floating_panes_coordinates instead of stepping a tiled resize.
    floating: bool,
    /// Animation frontier for the floating width.
    anim_width: usize,
    selectable: bool,
    role: Role,
    /// Header text override; None shows the session name.
    header: Option<String>,
    /// Dock: column where the dock is placed (= handle width).
    dock_x: usize,
    /// Handle: a dock instance currently exists in our tab.
    dock_open: bool,
    /// Handle: plugin url used to launch the dock (its own url is not
    /// exposed to a plugin, so the layout passes it).
    dock_url: String,
    /// Handle: dock options forwarded from our own config.
    dock_cfg: BTreeMap<String, String>,
    /// Handle: rows show tab names instead of icon + pane count. `names_pinned`
    /// is the Cmd+B state; `names_hover` is transient while the mouse is over.
    names_pinned: bool,
    names_hover: bool,
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
        self.hover_expand = true;
        self.zellij_bin = "zellij".into();
        self.selectable = false;
        self.anim_width = DEFAULT_FULL_WIDTH;
        self.dock_x = 4;
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
                "hover_expand" => self.hover_expand = on,
                "zellij_bin" => self.zellij_bin = v.clone(),
                "selectable" | "keyboard" => self.selectable = on,
                "role" => {
                    self.role = match v.as_str() {
                        "handle" => Role::Handle,
                        "dock" => Role::Dock,
                        _ => Role::Rail,
                    }
                }
                "header" => self.header = Some(v.clone()),
                "dock_x" => {
                    if let Ok(x) = v.parse::<usize>() {
                        self.dock_x = x;
                    }
                }
                "dock_url" => self.dock_url = v.clone(),
                "width" => {
                    if let Ok(w) = v.parse::<usize>() {
                        self.full_width = w.max(MINI_COLS + 1);
                    }
                }
                "start_minimized" if on => self.mode = Mode::Mini,
                _ => {}
            }
        }
        for k in [
            "width",
            "header",
            "auto_rename",
            "auto_expand",
            "show_tree",
            "show_header",
            "zellij_bin",
        ] {
            if let Some(v) = cfg.get(k) {
                self.dock_cfg.insert(k.to_string(), v.clone());
            }
        }
        self.dock_cfg.insert("role".into(), "dock".into());
        // hover events only reach a pane that can be a mouse target
        self.dock_cfg.insert("selectable".into(), "true".into());
        self.dock_cfg
            .insert("dock_x".into(), self.dock_x.to_string());
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
            // pipe_message_to_plugin (handle → dock) and unblock_cli_pipe_input
            PermissionType::MessageAndLaunchOtherPlugins,
            PermissionType::ReadCliPipes,
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
        // Non-selectable by default: the dock never steals focus, and zellij
        // closes the tab when the last shell exits without our help.
        set_selectable(self.selectable);
        self.plugin_id = get_plugin_ids().plugin_id;
        if self.full_width == 0 {
            self.full_width = DEFAULT_FULL_WIDTH;
        }
        self.anim_width = if self.mode == Mode::Mini {
            1
        } else {
            self.full_width
        };
        if self.role == Role::Dock {
            // The dock is always the full rail; visibility is the toggle. It is
            // launched at the handle's width and grows to `width` on screen.
            self.mode = Mode::Full;
            self.anim_width = 1;
            // Arm the peek timer at once: a dock nobody refreshes closes itself.
            self.peek = true;
            self.hover_seen = true;
            self.arm(PEEK_SECS);
        }
        subscribe(&[EventType::Visible]);
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
                self.note_bells();
                self.maybe_rename();
                true
            }
            Event::PaneUpdate(m) => {
                self.panes = m.panes;
                self.floating = self
                    .panes
                    .values()
                    .flatten()
                    .any(|p| p.is_plugin && p.id == self.plugin_id && p.is_floating);
                if self.role == Role::Handle {
                    self.dock_open = self.own_tab().is_some_and(|(_, ps)| {
                        ps.iter().any(|p| {
                            p.is_plugin
                                && p.is_floating
                                && p.id != self.plugin_id
                                && p.plugin_url
                                    .as_deref()
                                    .is_some_and(|u| u.contains("zjsidetabs"))
                        })
                    });
                }
                self.close_if_orphaned();
                self.maybe_rename();
                true
            }
            Event::Key(k) => self.key(k),
            Event::Mouse(m) => self.mouse(m),
            Event::Timer(_) => {
                self.timer_armed = false;
                self.spinner = self.spinner.wrapping_add(1);
                if self.role == Role::Dock && self.peek && !self.hover_seen {
                    close_self();
                    return false;
                }
                if self.role == Role::Handle && self.names_hover {
                    if self.hover_seen {
                        self.hover_seen = false;
                        self.arm(PEEK_SECS);
                    } else {
                        self.names_hover = false;
                        self.hover = None;
                    }
                }
                self.flash.retain(|_, n| {
                    *n -= 1;
                    *n > 0
                });
                if self.peek {
                    if self.hover_seen {
                        self.hover_seen = false;
                        self.arm(PEEK_SECS);
                    } else {
                        self.peek = false;
                        self.set_mode(Mode::Mini);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn pipe(&mut self, msg: PipeMessage) -> bool {
        if matches!(msg.source, PipeSource::Cli(_)) {
            unblock_cli_pipe_input(&msg.name);
        }
        if msg.name == "zjsidetabs" && self.role != Role::Rail {
            let cmd = msg.payload.as_deref().map(str::trim).unwrap_or("toggle");
            if self.role == Role::Handle {
                // Broadcasts reach every tab's handle; only the visible one acts.
                let own_active = self
                    .own_tab()
                    .and_then(|(pos, _)| self.tabs.iter().find(|t| t.position == pos))
                    .is_some_and(|t| t.active);
                if !own_active {
                    return false;
                }
                match cmd {
                    "toggle" => self.names_pinned = !self.names_pinned,
                    "show" | "max" | "expand" | "names" => self.names_pinned = true,
                    "hide" | "min" | "collapse" | "compact" => self.names_pinned = false,
                    _ => return false,
                }
                return true;
            }
            // Dock
            match cmd {
                "peek" => {
                    self.peek = true;
                    self.hover_seen = true;
                    self.arm(PEEK_SECS);
                }
                "show" => self.peek = false,
                "hide" => close_self(),
                _ => return false,
            }
            return true;
        }
        if msg.name == "zjsidetabs" {
            let target = match msg.payload.as_deref().map(str::trim).unwrap_or("toggle") {
                "toggle" => match self.mode {
                    Mode::Full => Mode::Mini,
                    Mode::Mini => Mode::Full,
                },
                "min" | "minimize" | "collapse" | "hide" => Mode::Mini,
                "max" | "expand" | "show" => Mode::Full,
                _ => return false,
            };
            self.peek = false;
            self.set_mode(target);
            return true;
        }
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
        if self.role == Role::Handle {
            self.render_handle(rows, cols);
            return;
        }
        if self.role == Role::Dock {
            self.drive_floating();
        } else {
            self.drive_resize();
        }
        if self.mode == Mode::Full && cols > MINI_COLS && self.resize_from.is_none() {
            self.full_width = cols;
        }
        if cols <= MINI_COLS || self.mode == Mode::Mini {
            self.render_mini(rows, cols);
            return;
        }
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
            rename: self.rename.as_ref().map(|(p, b)| (*p, b.as_str())),
            header: self.header.as_deref(),
            flash: &self.flash,
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
            .any(|r| matches!(r, Row::Activity { glyph, .. } if glyph == "\u{25b6}"))
            || !self.flash.is_empty();
        if running && !self.timer_armed {
            self.timer_armed = true;
            set_timeout(SPINNER_SECS);
        }
    }
}

impl State {
    /// (position, panes) of the tab this instance lives in, or None before
    /// the first PaneUpdate that includes us.
    fn own_tab(&self) -> Option<(usize, &Vec<PaneInfo>)> {
        self.panes
            .iter()
            .find(|(_, ps)| ps.iter().any(|p| p.is_plugin && p.id == self.plugin_id))
            .map(|(pos, ps)| (*pos, ps))
    }

    /// zellij does not reap a tab whose last pane was a plugin closing
    /// itself, so once our shells are gone we close the tab, not just us.
    fn close_if_orphaned(&mut self) {
        let Some((pos, panes)) = self.own_tab() else {
            return;
        };
        let terminals = panes
            .iter()
            .filter(|p| !p.is_plugin && !p.is_suppressed)
            .count();
        if terminals > 0 {
            self.had_terminal = true;
            return;
        }
        if !self.had_terminal {
            return;
        }
        match self.tabs.iter().find(|t| t.position == pos) {
            Some(t) => close_tab_with_id(t.tab_id as u64),
            None => close_self(),
        }
    }

    /// Rising edge of a bell on a background tab starts a flash.
    fn note_bells(&mut self) {
        for t in &self.tabs {
            if t.has_bell_notification && !t.active {
                if !self.bell_seen.contains(&t.position) {
                    self.bell_seen.insert(t.position);
                    self.flash.insert(t.position, FLASH_TICKS);
                }
            } else {
                self.bell_seen.remove(&t.position);
            }
        }
    }

    fn begin_rename(&mut self, pos: usize) {
        let current = self
            .tabs
            .iter()
            .find(|t| t.position == pos)
            .map(|t| {
                if model::is_default_name(&t.name) {
                    String::new()
                } else {
                    t.name.clone()
                }
            })
            .unwrap_or_default();
        self.rename = Some((pos, current));
    }

    fn commit_rename(&mut self) {
        let Some((pos, buf)) = self.rename.take() else {
            return;
        };
        let name = buf.trim();
        if name.is_empty() {
            return;
        }
        if let Some(t) = self.tabs.iter().find(|t| t.position == pos) {
            rename_tab_with_id(t.tab_id as u64, name);
            // A typed name is the user's: never auto-rename over it again.
            self.our_names.remove(&pos);
        }
    }

    /// Move tab `from` to `to` by focusing it and stepping with the CLI;
    /// 0.45 exposes no move-tab command to plugins.
    fn reorder(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        switch_tab_to(from as u32 + 1);
        let dir = if to > from { "right" } else { "left" };
        for _ in 0..from.abs_diff(to) {
            run_command(
                &[&self.zellij_bin, "action", "move-tab", dir],
                BTreeMap::new(),
            );
        }
    }

    fn arm(&mut self, secs: f64) {
        if !self.timer_armed {
            self.timer_armed = true;
            set_timeout(secs);
        }
    }

    fn set_mode(&mut self, m: Mode) {
        if m == self.mode {
            return;
        }
        self.mode = m;
        self.cursor = None;
        self.hover = None;
        self.resize_from = None;
    }

    /// One resize step per render toward the current mode's width. zellij
    /// re-renders after every resize, so this converges on its own — and
    /// the steps are the collapse animation.
    fn drive_resize(&mut self) {
        if self.floating {
            self.drive_floating();
            return;
        }
        let want_shrink = self.mode == Mode::Mini && self.cols > 1;
        let want_grow = self.mode == Mode::Full && self.cols < self.full_width;
        if !want_shrink && !want_grow {
            self.resize_from = None;
            return;
        }
        if self.resize_from == Some(self.cols) {
            return;
        }
        self.resize_from = Some(self.cols);
        let resize = if want_shrink {
            Resize::Decrease
        } else {
            Resize::Increase
        };
        resize_pane_with_id(
            ResizeStrategy {
                resize,
                direction: Some(Direction::Right),
                invert_on_boundaries: false,
            },
            PaneId::Plugin(self.plugin_id),
        );
    }

    /// Floating dock: move the width toward the target a slice per frame.
    /// Each call to change_floating_panes_coordinates triggers a re-render,
    /// so this converges in a few frames and reads as a slide.
    fn drive_floating(&mut self) {
        let target = if self.mode == Mode::Mini {
            1
        } else {
            self.full_width
        };
        if self.anim_width == target && self.cols == target {
            return;
        }
        let step = (self.full_width / 4).max(4);
        self.anim_width = if self.anim_width < target {
            (self.anim_width + step).min(target)
        } else {
            self.anim_width.saturating_sub(step).max(target)
        };
        change_floating_panes_coordinates(vec![(
            PaneId::Plugin(self.plugin_id),
            FloatingPaneCoordinates {
                x: Some(PercentOrFixed::Fixed(if self.role == Role::Dock {
                    self.dock_x
                } else {
                    0
                })),
                y: Some(PercentOrFixed::Fixed(0)),
                width: Some(PercentOrFixed::Fixed(self.anim_width)),
                height: Some(PercentOrFixed::Percent(100)),
                pinned: Some(true),
                borderless: Some(true),
            },
        )]);
    }

    /// The handle: a thin tiled strip with one row per tab showing its
    /// number. Hovering it summons the dock.
    fn render_handle(&mut self, rows: usize, cols: usize) {
        let p = &self.pal;
        let w = cols.max(1);
        let names = self.names_pinned || self.names_hover;
        let mut lines: Vec<String> = Vec::new();
        let mut map: Vec<Row> = Vec::new();
        let pad = |txt: &str| -> String {
            let n = theme::width(txt);
            format!("{txt}{}", " ".repeat(w.saturating_sub(n)))
        };
        if self.show_header {
            let title = if names {
                self.header
                    .clone()
                    .or_else(|| self.session.clone())
                    .unwrap_or_default()
            } else {
                "\u{2261}".to_string()
            };
            let title = render::fit(&title, w.saturating_sub(2));
            lines.push(theme::render(&pad(&format!(" #[fg={}]{title}", p.dim))));
            map.push(Row::Header);
        }
        for t in &self.tabs {
            let idx = t.position + 1;
            let panes: Vec<PaneInfo> = self.panes.get(&t.position).cloned().unwrap_or_default();
            let live = panes
                .iter()
                .filter(|p| !p.is_plugin && p.is_selectable && !p.is_suppressed)
                .count();
            let label = model::label(t, &panes);
            let flashing = self.flash.get(&t.position).is_some_and(|n| n % 2 == 0);
            let (bar, c_idx, c_aux, bold) = if flashing {
                (
                    format!("#[fg={}]\u{258c}", p.warn),
                    &p.warn,
                    &p.warn,
                    ",bold",
                )
            } else if t.active {
                (
                    format!("#[bg={}]#[fg={}]\u{258c}", p.surface, p.accent),
                    &p.text,
                    &p.accent,
                    ",bold",
                )
            } else if t.has_bell_notification {
                (" ".into(), &p.warn, &p.warn, "")
            } else if self.hover == Some(map.len()) {
                (" ".into(), &p.text, &p.subtext, "")
            } else {
                (" ".into(), &p.subtext, &p.dim, "")
            };
            // rest:  ▌ 2 󰄛 3      hover/pinned:  ▌ 2 gitlab
            let line = if names {
                let room = w.saturating_sub(5);
                format!(
                    "{bar}#[fg={c_idx}{bold}]{idx:>2} #[fg={c_idx}{bold}]{}",
                    render::fit(&label.text, room)
                )
            } else {
                let count = if live > 1 {
                    format!(" #[fg={c_aux}]{live}")
                } else {
                    String::new()
                };
                format!(
                    "{bar}#[fg={c_idx}{bold}]{idx:>2} #[fg={c_aux}]{}{count}",
                    label.icon
                )
            };
            lines.push(theme::render(&pad(&line)));
            map.push(Row::Tab { pos: t.position });
        }
        let mut out = String::new();
        for i in 0..rows {
            let l = lines.get(i).cloned().unwrap_or_default();
            out.push_str(&l);
            for _ in theme::width(&l)..w {
                out.push(' ');
            }
            if i + 1 < rows {
                out.push('\n');
            }
        }
        print!("{out}");
        self.rows = map;
    }

    /// The minimized band: one glyph per tab. Active in accent, bell in
    /// warn, the rest dim. Rows map 1:1 so clicks still hit a tab.
    fn render_mini(&mut self, rows: usize, cols: usize) {
        let p = &self.pal;
        let mut lines: Vec<String> = Vec::new();
        let mut map: Vec<Row> = Vec::new();
        if self.show_header {
            lines.push(theme::render(&format!("#[fg={}]\u{2594}", p.dim)));
            map.push(Row::Header);
        }
        for t in &self.tabs {
            let g = if t.active {
                format!("#[fg={}]\u{2588}", p.accent)
            } else if t.has_bell_notification {
                format!("#[fg={}]\u{25cf}", p.warn)
            } else {
                format!("#[fg={}]\u{25aa}", p.dim)
            };
            lines.push(theme::render(&g));
            map.push(Row::Tab { pos: t.position });
        }
        let mut out = String::new();
        for i in 0..rows {
            let l = lines.get(i).cloned().unwrap_or_default();
            out.push_str(&l);
            for _ in theme::width(&l)..cols.max(1) {
                out.push(' ');
            }
            if i + 1 < rows {
                out.push('\n');
            }
        }
        print!("{out}");
        self.rows = map;
    }

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

    /// Give a default-named tab a name from what runs in it. Only this
    /// instance's own tab: one rail per tab means one writer per name, so
    /// instances never fight and a stale build in another tab cannot
    /// rewrite this one.
    fn maybe_rename(&mut self) {
        if !self.auto_rename {
            return;
        }
        let Some((pos, panes)) = self.own_tab().map(|(p, ps)| (p, ps.clone())) else {
            return;
        };
        let Some(tab) = self.tabs.iter().find(|t| t.position == pos) else {
            return;
        };
        let ours = self.our_names.get(&pos).is_some_and(|n| *n == tab.name);
        if !model::is_default_name(&tab.name) && !ours {
            return;
        }
        let l = model::label(tab, &panes);
        if l.derived && !model::is_default_name(&l.text) && l.text != tab.name {
            rename_tab_with_id(tab.tab_id as u64, &l.text);
            self.our_names.insert(pos, l.text);
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
        // Typing into the rail means the user wants it: stop treating the
        // expansion as a hover peek.
        self.peek = false;
        if let Some((_, buf)) = self.rename.as_mut() {
            match k.bare_key {
                BareKey::Esc => self.rename = None,
                BareKey::Enter => self.commit_rename(),
                BareKey::Backspace => {
                    buf.pop();
                }
                BareKey::Char(c) if !k.key_modifiers.contains(&KeyModifier::Ctrl) => buf.push(c),
                _ => return false,
            }
            return true;
        }
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
            BareKey::Char('b') | BareKey::Char('-') => self.set_mode(Mode::Mini),
            BareKey::Char('r') => {
                if let Some(t) = self.cursor.and_then(|c| self.row_tab(c)) {
                    self.begin_rename(t);
                }
            }
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
        if self.role == Role::Handle {
            return match m {
                Mouse::Hover(line, _) => {
                    if self.hover_expand && !self.names_hover {
                        self.names_hover = true;
                    }
                    self.hover_seen = true;
                    self.arm(PEEK_SECS);
                    let h = usize::try_from(line)
                        .ok()
                        .filter(|i| matches!(self.rows.get(*i), Some(Row::Tab { .. })));
                    if h != self.hover {
                        self.hover = h;
                        return true;
                    }
                    false
                }
                Mouse::LeftClick(line, _) => {
                    if let Some(Row::Tab { pos }) = usize::try_from(line)
                        .ok()
                        .and_then(|i| self.rows.get(i).cloned())
                    {
                        switch_tab_to(pos as u32 + 1);
                    }
                    true
                }
                Mouse::RightClick(..) => {
                    self.names_pinned = !self.names_pinned;
                    true
                }
                Mouse::ScrollUp(_) => {
                    self.step_tab(false);
                    true
                }
                Mouse::ScrollDown(_) => {
                    self.step_tab(true);
                    true
                }
                _ => false,
            };
        }
        // Dock: a peeked dock closes once you've picked something.
        if self.role == Role::Dock {
            if let Mouse::LeftClick(line, _) = m {
                let was_peek = self.peek;
                if let Ok(i) = usize::try_from(line) {
                    if matches!(
                        self.rows.get(i),
                        Some(Row::Tab { .. } | Row::Pane { .. } | Row::Activity { .. })
                    ) {
                        self.activate(i);
                        if was_peek {
                            close_self();
                        }
                        return true;
                    }
                }
            }
        }
        match m {
            Mouse::LeftClick(line, col) => {
                let Ok(i) = usize::try_from(line) else {
                    return false;
                };
                // A click pins a hover-peeked rail open.
                self.peek = false;
                self.drag = match self.rows.get(i) {
                    Some(Row::Tab { pos }) => Some((i, *pos)),
                    _ => None,
                };
                if self.mode == Mode::Mini {
                    if let Some(Row::Tab { pos }) = self.rows.get(i).cloned() {
                        switch_tab_to(pos as u32 + 1);
                    }
                    self.set_mode(Mode::Full);
                    return true;
                }
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
            Mouse::Hold(line, _) => {
                if self.drag.is_some() {
                    let h = usize::try_from(line)
                        .ok()
                        .filter(|i| matches!(self.rows.get(*i), Some(Row::Tab { .. })));
                    if h != self.hover {
                        self.hover = h;
                        return true;
                    }
                }
                false
            }
            Mouse::Release(line, _) => {
                let Some((from_row, from_pos)) = self.drag.take() else {
                    return false;
                };
                let Ok(i) = usize::try_from(line) else {
                    return false;
                };
                if i != from_row {
                    if let Some(Row::Tab { pos }) = self.rows.get(i).cloned() {
                        self.reorder(from_pos, pos);
                        return true;
                    }
                }
                false
            }
            Mouse::Hover(line, _) => {
                if self.mode == Mode::Mini && self.hover_expand {
                    self.set_mode(Mode::Full);
                    self.peek = true;
                    self.hover_seen = true;
                    self.arm(PEEK_SECS);
                    return true;
                }
                if self.peek {
                    self.hover_seen = true;
                }
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
