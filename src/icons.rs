//! Maps a pane's running command to a Nerd Font glyph.

/// Ordered longest-prefix-first so `docker-compose` wins over `docker`.
const MAP: &[(&str, &str)] = &[
    ("docker-compose", "\u{f308}"),
    ("kubectl", "\u{f10fe}"),
    ("claude", "\u{f0e59}"),
    ("nvim", "\u{e6ae}"),
    ("vim", "\u{e6ae}"),
    ("hx", "\u{e6ae}"),
    ("lazygit", "\u{e702}"),
    ("git", "\u{e702}"),
    ("docker", "\u{f308}"),
    ("k9s", "\u{f10fe}"),
    ("python", "\u{e235}"),
    ("node", "\u{e718}"),
    ("npm", "\u{e71e}"),
    ("cargo", "\u{e7a8}"),
    ("rustc", "\u{e7a8}"),
    ("ssh", "\u{f08c0}"),
    ("btop", "\u{f0c58}"),
    ("htop", "\u{f0c58}"),
    ("man", "\u{f02d}"),
    ("less", "\u{f02d}"),
    ("bash", "\u{f489}"),
    ("zsh", "\u{f489}"),
    ("fish", "\u{f489}"),
];

const DEFAULT: &str = "\u{f489}";

/// Pick an icon from a command line. Takes the basename of argv[0], then the
/// first argument when argv[0] is a runner (`sudo`, `env`, package managers).
pub fn for_command(cmd: Option<&str>) -> &'static str {
    let Some(cmd) = cmd else { return DEFAULT };
    let mut parts = cmd.split_whitespace().filter(|p| !p.contains('='));
    let Some(first) = parts.next() else {
        return DEFAULT;
    };
    let base = basename(first);

    if matches!(base, "sudo" | "env" | "npx" | "uv" | "uvx" | "bunx") {
        if let Some(next) = parts.next() {
            return lookup(basename(next));
        }
    }
    lookup(base)
}

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn lookup(name: &str) -> &'static str {
    MAP.iter()
        .find(|(k, _)| name == *k || name.starts_with(k))
        .map(|(_, v)| *v)
        .unwrap_or(DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_command() {
        assert_eq!(for_command(Some("nvim")), "\u{e6ae}");
    }

    #[test]
    fn absolute_path_is_stripped() {
        assert_eq!(
            for_command(Some("/opt/homebrew/bin/nvim src/x.rs")),
            "\u{e6ae}"
        );
    }

    #[test]
    fn runner_prefix_is_skipped() {
        assert_eq!(for_command(Some("sudo docker ps")), "\u{f308}");
    }

    #[test]
    fn env_assignments_are_ignored() {
        assert_eq!(for_command(Some("FOO=1 git status")), "\u{e702}");
    }

    #[test]
    fn longest_prefix_wins() {
        assert_eq!(for_command(Some("docker-compose up")), "\u{f308}");
    }

    #[test]
    fn unknown_and_none_fall_back() {
        assert_eq!(for_command(Some("wat")), DEFAULT);
        assert_eq!(for_command(None), DEFAULT);
    }
}
