// A Claude Code status line (https://code.claude.com/docs/en/statusline),
// styled after Pure (https://github.com/sindresorhus/pure).
use serde_json::Value;
use std::io::{self, Read, Write};
use std::path::Path;

const GREY: &str = "\x1b[38;5;242m";
const BLUE: &str = "\x1b[34m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const PINK: &str = "\x1b[38;5;218m";
const RESET: &str = "\x1b[0m";

/// Drop a trailing `.0` from a one-decimal string.
fn strip_zero_fraction(x: f64) -> String {
    let s = format!("{x:.1}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_string)
}

/// Format a token count with k/M suffixes.
fn format_token_count(count: i64) -> String {
    if count < 1_000 {
        return count.to_string();
    }
    // Round to one decimal before testing the M threshold, so that 999_950
    // becomes "1M" instead of "1000k".
    let k = (count as f64 / 100.0).round() / 10.0;
    if k < 1_000.0 {
        return format!("{}k", strip_zero_fraction(k));
    }
    let m = (count as f64 / 100_000.0).round() / 10.0;
    format!("{}M", strip_zero_fraction(m))
}

/// Abbreviate `home` to `~`.
fn tildify(p: &str, home: &str) -> String {
    // An empty `home` would trivially be a prefix of every path.
    if home.is_empty() {
        return p.to_string();
    }
    match Path::new(p).strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => Path::new("~").join(rest).display().to_string(),
        Err(_) => p.to_string(),
    }
}

fn current_dir(v: &Value) -> &str {
    v["workspace"]["current_dir"]
        .as_str()
        .or_else(|| v["cwd"].as_str())
        .unwrap_or("")
}

/// Run `git` in `dir` and return its trimmed stdout on success.
fn git(dir: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", dir])
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let s = String::from_utf8(out.stdout).ok()?;
    Some(s.trim().to_string())
}

#[derive(Debug, Default, PartialEq)]
struct GitInfo {
    head: String,
    unstaged: bool,
    staged: bool,
    untracked: bool,
    ahead: bool,
    behind: bool,
}

fn parse_status(s: &str) -> GitInfo {
    let mut info = GitInfo::default();
    for line in s.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            info.head = head.to_string();
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut ab = ab.split_whitespace();
            info.ahead = ab.next().is_some_and(|n| n != "+0");
            info.behind = ab.next().is_some_and(|n| n != "-0");
        } else if let Some(entry) = line
            .strip_prefix("1 ")
            .or_else(|| line.strip_prefix("2 "))
            .or_else(|| line.strip_prefix("u "))
        {
            let mut xy = entry.chars();
            info.staged |= xy.next().is_some_and(|c| c != '.');
            info.unstaged |= xy.next().is_some_and(|c| c != '.');
        } else if line.starts_with('?') {
            info.untracked = true;
        }
    }
    info
}

fn git_info(dir: &str) -> Option<GitInfo> {
    if dir.is_empty() {
        return None;
    }
    // `--no-optional-locks` stops the refresh loop from taking the index
    // lock that concurrent `git` commands need.
    let status = git(
        dir,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
        ],
    )?;
    let mut info = parse_status(&status);
    if info.head == "(detached)" {
        info.head = git(
            dir,
            &[
                "name-rev",
                "--name-only",
                "--no-undefined",
                "--always",
                "HEAD",
            ],
        )?;
    }
    (!info.head.is_empty()).then_some(info)
}

fn render(v: &Value, home: &str, git: Option<&GitInfo>) -> String {
    let model = v["model"]["display_name"].as_str().unwrap_or("");
    let cwd = tildify(current_dir(v), home);

    let mut segs: Vec<String> = Vec::new();
    if !model.is_empty() {
        segs.push(format!("{GREY}{model}{RESET}"));
    }
    segs.push(format!("{BLUE}{cwd}{RESET}"));

    if let Some(git) = git {
        let mut head = format!("{GREY}{}{RESET}", git.head);
        let markers = format!(
            "{}{}{}",
            if git.unstaged { "*" } else { "" },
            if git.staged { "+" } else { "" },
            if git.untracked { "?" } else { "" }
        );
        if !markers.is_empty() {
            head.push_str(&format!("{PINK}{markers}{RESET}"));
        }
        segs.push(head);
        let arrows = format!(
            "{}{}",
            if git.behind { "⇣" } else { "" },
            if git.ahead { "⇡" } else { "" }
        );
        if !arrows.is_empty() {
            segs.push(format!("{CYAN}{arrows}{RESET}"));
        }
    }

    let ctx = &v["context_window"];
    if let (Some(used), Some(size)) = (
        ctx["total_input_tokens"].as_i64(),
        ctx["context_window_size"].as_i64(),
    ) {
        if size > 0 {
            let pct = (used as f64 * 100.0 / size as f64).round() as i64;
            segs.push(format!(
                "{YELLOW}{}/{} ({pct}%){RESET}",
                format_token_count(used),
                format_token_count(size)
            ));
        }
    }

    segs.join(" ")
}

fn main() {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(&input) else {
        return;
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let git = git_info(current_dir(&v));
    let _ = io::stdout().write_all(render(&v, &home, git.as_ref()).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_token_count_cases() {
        let cases = [
            (0, "0"),
            (999, "999"),
            (1_000, "1k"),
            (1_500, "1.5k"),
            (9_999, "10k"),
            (10_000, "10k"),
            (12_345, "12.3k"),
            (99_999, "100k"),
            (200_000, "200k"),
            (999_499, "999.5k"),
            (999_500, "999.5k"),
            (999_949, "999.9k"),
            (999_950, "1M"),
            (999_999, "1M"),
            (1_000_000, "1M"),
            (1_500_000, "1.5M"),
            (9_999_999, "10M"),
            (10_000_000, "10M"),
            (99_999_999, "100M"),
        ];
        for (input, expected) in cases {
            assert_eq!(format_token_count(input), expected, "input = {input}");
        }
    }

    #[test]
    fn parse_status_cases() {
        let cases = [
            (
                "clean and in sync",
                "# branch.oid 4ae0e0f\n\
                 # branch.head master\n\
                 # branch.upstream origin/master\n\
                 # branch.ab +0 -0",
                GitInfo {
                    head: "master".to_string(),
                    ..Default::default()
                },
            ),
            (
                "unstaged modification on a diverged branch",
                "# branch.oid 4ae0e0f\n\
                 # branch.head master\n\
                 # branch.upstream origin/master\n\
                 # branch.ab +2 -1\n\
                 1 .M N... 100644 100644 100644 4ae0e0f 4ae0e0f src/main.rs",
                GitInfo {
                    head: "master".to_string(),
                    unstaged: true,
                    ahead: true,
                    behind: true,
                    ..Default::default()
                },
            ),
            (
                "staged rename",
                "# branch.oid 4ae0e0f\n\
                 # branch.head master\n\
                 2 R. N... 100644 100644 100644 4ae0e0f 4ae0e0f R100 new.rs\told.rs",
                GitInfo {
                    head: "master".to_string(),
                    staged: true,
                    ..Default::default()
                },
            ),
            (
                "merge conflict is both staged and unstaged",
                "# branch.oid 4ae0e0f\n\
                 # branch.head master\n\
                 u UU N... 100644 100644 100644 100644 4ae0e0f 4ae0e0f 4ae0e0f src/main.rs",
                GitInfo {
                    head: "master".to_string(),
                    unstaged: true,
                    staged: true,
                    ..Default::default()
                },
            ),
            (
                "untracked file",
                "# branch.oid 4ae0e0f\n# branch.head master\n? scratch.txt",
                GitInfo {
                    head: "master".to_string(),
                    untracked: true,
                    ..Default::default()
                },
            ),
            (
                "no upstream",
                "# branch.oid 4ae0e0f\n# branch.head topic",
                GitInfo {
                    head: "topic".to_string(),
                    ..Default::default()
                },
            ),
            (
                "detached HEAD",
                "# branch.oid 4ae0e0f\n# branch.head (detached)",
                GitInfo {
                    head: "(detached)".to_string(),
                    ..Default::default()
                },
            ),
        ];
        for (name, s, expected) in cases {
            assert_eq!(parse_status(s), expected, "{name}");
        }
    }

    #[test]
    fn render_cases() {
        let cases = [
            (
                "renders the full line",
                json!({
                    "workspace": { "current_dir": "/home/u/proj" },
                    "model": { "display_name": "Opus 4.8 (1M context)" },
                    "context_window": { "total_input_tokens": 44_000, "context_window_size": 1_000_000 }
                }),
                "/home/u",
                Some(GitInfo {
                    head: "master".to_string(),
                    unstaged: true,
                    staged: true,
                    untracked: true,
                    ahead: true,
                    ..Default::default()
                }),
                format!(
                    "{GREY}Opus 4.8 (1M context){RESET} \
                     {BLUE}~/proj{RESET} \
                     {GREY}master{RESET}{PINK}*+?{RESET} \
                     {CYAN}⇡{RESET} \
                     {YELLOW}44k/1M (4%){RESET}"
                ),
            ),
            (
                "renders a clean, diverged branch",
                json!({ "cwd": "/home/u/proj" }),
                "/home/u",
                Some(GitInfo {
                    head: "main".to_string(),
                    ahead: true,
                    behind: true,
                    ..Default::default()
                }),
                format!("{BLUE}~/proj{RESET} {GREY}main{RESET} {CYAN}⇣⇡{RESET}"),
            ),
            (
                "renders an in-sync branch without arrows",
                json!({ "cwd": "/home/u/proj" }),
                "/home/u",
                Some(GitInfo {
                    head: "main".to_string(),
                    ..Default::default()
                }),
                format!("{BLUE}~/proj{RESET} {GREY}main{RESET}"),
            ),
            (
                "omits absent segments and keeps cwd",
                json!({ "cwd": "/tmp/scratch" }),
                "/home/u",
                None,
                format!("{BLUE}/tmp/scratch{RESET}"),
            ),
            (
                "skips context when size is zero",
                json!({
                    "cwd": "/tmp",
                    "context_window": { "total_input_tokens": 10, "context_window_size": 0 }
                }),
                "/home/u",
                None,
                format!("{BLUE}/tmp{RESET}"),
            ),
            (
                "leaves cwd outside home untouched",
                json!({ "cwd": "/var/data" }),
                "/home/u",
                None,
                format!("{BLUE}/var/data{RESET}"),
            ),
            (
                "abbreviates home itself",
                json!({ "cwd": "/home/u" }),
                "/home/u",
                None,
                format!("{BLUE}~{RESET}"),
            ),
            (
                "abbreviates only at a path boundary",
                json!({ "cwd": "/home/username" }),
                "/home/u",
                None,
                format!("{BLUE}/home/username{RESET}"),
            ),
            (
                "tolerates a trailing slash in home",
                json!({ "cwd": "/home/u/proj" }),
                "/home/u/",
                None,
                format!("{BLUE}~/proj{RESET}"),
            ),
            (
                "does not abbreviate when home is empty",
                json!({ "cwd": "/home/u/proj" }),
                "",
                None,
                format!("{BLUE}/home/u/proj{RESET}"),
            ),
        ];
        for (name, v, home, git, expected) in cases {
            assert_eq!(render(&v, home, git.as_ref()), expected, "{name}");
        }
    }
}
