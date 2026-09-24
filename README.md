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

The rail is one fixed-width pane. Each row is a tab:

```
 sidebar
 󰆍 ⌘1
 󰅩 ⌘2 3          ← icon of what runs there · the tab's Cmd shortcut · pane count (dim)
▌󰆍 ⌘3            ← active tab
```

Move the mouse over it and the rows switch to names — `󰅩 ⌘2 gitlab      3` — and switch back about a second after the mouse leaves. Right-click, or a `toggle` message (Cmd+B below), pins the names view. Nothing is launched or resized, so nothing flickers.

- **Icons per program** — Nerd Font glyph picked from the pane's command (nvim, git, docker, node, cargo, claude, k8s, ssh, …), seeing through `sudo`/`env`/`npx`.
- **Meaningful names** — a tab you haven't named shows its program, or the cwd for a plain shell, and follows changes until you rename it yourself.
- **Click** a row to switch. **Scroll** over the rail to step through tabs. A tab whose pane rings a bell pulses orange.
- **Full rail mode** (`role "rail"`, the default): the wider layout with a foldable pane tree under each tab, `/` filter, `r` inline rename, drag-to-reorder, pipe-fed activity rows. See the tiled layout below.

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

**Handle (recommended).** A 14-column strip; hover shows names.

```kdl
// ~/.config/zellij/layouts/sidetabs.kdl
layout {
    pane split_direction="vertical" {
        pane size=14 borderless=true {
            plugin location="file:~/.config/zellij/plugins/zjsidetabs.wasm" {
                role        "handle"
                selectable  "true"
                header      "sidebar"
                auto_rename "true"
            }
        }
        pane
    }
    pane size=1 borderless=true {
        plugin location="zellij:compact-bar"
    }
}
```

**Full rail.** The wide sidebar with pane tree, filter and rename.

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

Then `default_layout "sidetabs"` in `config.kdl`. On first run zellij shows a y/n permission prompt inside the rail; click the rail and press `y` — it grants `ReadApplicationState`, `ChangeApplicationState`, `RunCommands`, `MessageAndLaunchOtherPlugins` and `ReadCliPipes` for the plugin file and remembers them.

> **Upgrading:** zellij compiles plugins once per *session* and keeps the module cached by path. After replacing `zjsidetabs.wasm`, start a new session (`zellij kill-session <name> && zellij delete-session <name>`, then reattach) — new tabs in a running session keep loading the old build.

## Options

| Key | Default | Meaning |
|---|---|---|
| `auto_expand` | `true` | Unfold the active tab automatically |
| `auto_rename` | `false` | Rename default-named tabs to their running program |
| `show_header` | `true` | Session name at the top |
| `show_tree` | `true` | Show panes under unfolded tabs |
| `role` | `rail` | `rail` (full sidebar) or `handle` (strip with hover names) |
| `hover_expand` | `true` | Handle: show names while the mouse is over the rail |
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
