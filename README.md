# zjsidetabs

A vertical, foldable tab rail for [zellij](https://zellij.dev). Tabs on the left, like a browser sidebar — with the things a horizontal tab bar can't do.

```
   main

  ▾ 1  claude              ●
      ├  claude ●
      └  zsh
  ▸ 2  nvim                  3
    3  docker
      ◐ run tests
```

## What it does

- **Fold / unfold** — each tab is a folder. Expand it to see its panes as a tree; click a pane to focus it. The active tab unfolds itself; fold it back with a right-click, the chevron, or `Space`.
- **Icons per program** — Nerd Font glyph picked from what runs in the pane (nvim, git, docker, node, cargo, claude, k8s, ssh, …), seeing through `sudo`/`env`/`npx`.
- **Meaningful titles** — a tab you haven't named shows the program running in it. With `auto_rename`, zellij's `Tab #3` is renamed to that program, and keeps following it until you rename the tab yourself.
- **Filter / jump** — press `/`, type, `Enter` jumps to the first match. Matches tab names, pane titles and commands.
- **Activity rows** — live sub-rows under a tab fed over `zellij pipe`, compatible with the [cfal/zellij-vertical-tabs](https://github.com/cfal/zellij-vertical-tabs) payload, so existing Claude Code hooks work unchanged. In-progress items get a spinner.
- **Badges** — bell, fullscreen, synced input, folded pane count.
- **Mouse** — click to switch/focus, hover highlight, scroll to move through tabs (or through the list when it overflows).
- **Minimize** — collapse the whole rail to a 1-column band (one glyph per tab: active in accent, bell in orange) and back. The resize is stepped, so it animates. Trigger it from a keybind via `MessagePlugin` (below), with `b` / `-` in the rail, or by clicking the band.
- **Hover to peek** — while minimized, mousing over the band expands the rail; moving away collapses it again ~half a second later. A click, a keypress, or the toggle pins it open. `hover_expand "false"` turns this off.
- **Rename inline** — `r` on a tab opens an editor in the row; `Enter` saves, `Esc` cancels. A typed name is yours: auto-rename never touches it again.
- **Drag to reorder** — press on a tab, release on another. (Goes through `zellij action move-tab`; set `zellij_bin` if `zellij` isn't on the server's PATH.)
- **Bell flash** — a background tab whose pane rings pulses orange for a second, then keeps the bell badge.
- **Keyboard** (when the rail is focused) — `j`/`k` move, `Enter` activate, `l` unfold-or-activate, `h`/`Space` fold, `z` fold/unfold all, `g`/`G` first/last, `r` rename, `/` filter, `b`/`-` minimize, `Esc` clear.

## Install

Grab `zjsidetabs.wasm` from the [latest release](https://github.com/kannonski/zjsidetabs/releases/latest):

```sh
mkdir -p ~/.config/zellij/plugins
curl -L -o ~/.config/zellij/plugins/zjsidetabs.wasm \
  https://github.com/kannonski/zjsidetabs/releases/latest/download/zjsidetabs.wasm
```

Or build it — needs Rust and the `wasm32-wasip1` target:

```sh
rustup target add wasm32-wasip1
cargo build --release
cp target/wasm32-wasip1/release/zjsidetabs.wasm ~/.config/zellij/plugins/
```

## Layout

Two ways to mount the rail.

**Floating dock (recommended).** A pinned overlay on the left edge: one column at rest, slides out on hover or on a keybind, never takes focus, and the terminal keeps its full width.

```kdl
// ~/.config/zellij/layouts/sidetabs.kdl
layout {
    pane
    floating_panes {
        pane {
            plugin location="file:~/.config/zellij/plugins/zjsidetabs.wasm" {
                auto_rename     "true"
                start_minimized "true"
                width           "30"
            }
            x 0
            y 0
            width 1
            height "100%"
            pinned true
            borderless true
        }
    }
    pane size=1 borderless=true {
        plugin location="zellij:compact-bar"
    }
}
```

**Tiled column.** A fixed sidebar that reserves its width. Minimize/expand isn't available in this mode — a layout `size` is a hard constraint and zellij refuses to resize it.

```kdl
layout {
    pane split_direction="vertical" {
        pane size=30 borderless=true {
            plugin location="file:~/.config/zellij/plugins/zjsidetabs.wasm" {
                auto_rename "true"
                selectable  "true"
            }
        }
        pane
    }
    pane size=1 borderless=true {
        plugin location="zellij:compact-bar"
    }
}
```

Then `default_layout "sidetabs"` in `config.kdl`. On first run zellij asks for `ReadApplicationState`, `ChangeApplicationState` and `RunCommands` (drag-reorder); focus the rail and press `y`.

## Options

| Key | Default | Meaning |
|---|---|---|
| `auto_expand` | `true` | Unfold the active tab automatically |
| `auto_rename` | `false` | Rename default-named tabs to their running program |
| `show_header` | `true` | Session name at the top |
| `show_tree` | `true` | Show panes under unfolded tabs |
| `hover_expand` | `true` | Minimized band expands on mouse-over, collapses when the mouse leaves |
| `start_minimized` | `false` | Start as the 1-column band |
| `zellij_bin` | `zellij` | Binary used for drag-reorder (`move-tab`) |
| `width` | `30` | Expanded width of the floating dock, in columns |
| `selectable` | `false` | Let the rail take keyboard focus (enables `j`/`k`, `/`, `r`). Off keeps a dock from stealing focus |
| `color_accent` | `#cba6f7` | Active index, chevron, focused-pane dot |
| `color_text` | `#cdd6f4` | Active / hovered label |
| `color_subtext` | `#a6adc8` | Inactive label |
| `color_muted` | `#6c7086` | Icons, activity text |
| `color_dim` | `#585b70` | Inactive index, tree lines, counts |
| `color_surface` | `#313244` | Active pill background |
| `color_surface_hi` | `#45475a` | Hover background |
| `color_warn` | `#fab387` | Bell badge |
| `color_ok` | `#a6e3a1` | Sync badge |

Colours take `#rrggbb`, `#rgb` or a 0–255 palette index. Defaults are Catppuccin Mocha.

## Toggling from a keybind

```kdl
// ~/.config/zellij/config.kdl
keybinds {
    normal {
        bind "Alt b" {
            MessagePlugin { name "zjsidetabs"; payload "toggle"; }   // also: min | max
        }
    }
}
```

Leave the plugin URL out. zellij matches a running plugin by URL **and** configuration; the rail is started from a layout with configuration, so a URL-targeted message never matches and launches a second instance. With no URL the message is broadcast to every running plugin — the rail answers, others ignore it — so all tabs collapse together.

## Feeding activity

```sh
zellij pipe --name activity -- '{
  "zsession": "main",
  "name": "claude",
  "todos": [
    { "status": "in_progress", "text": "run tests" },
    { "status": "pending",     "text": "write migration" }
  ]
}'
```

Rows attach to the tab whose name — or focused pane's program — equals `name`. Sub-agents (`"subagents": { "id": { "icon", "glyph", "title" } }`) take priority over todos; done todos are hidden. Send an empty payload to clear.

## License

MIT
