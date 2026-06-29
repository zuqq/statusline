// A Claude Code status line (https://code.claude.com/docs/en/statusline),
// styled after Pure (https://github.com/sindresorhus/pure).
use serde_json::Value;
use std::io::{self, Read, Write};
use std::path::Path;

const GREY: &str = "\x1b[38;5;242m";
const BLUE: &str = "\x1b[34m";
const YELLOW: &str = "\x1b[33m";
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

fn render(v: &Value, home: &str) -> String {
    let ws = &v["workspace"];
    let raw_cwd = ws["current_dir"]
        .as_str()
        .or_else(|| v["cwd"].as_str())
        .unwrap_or("");
    let model = v["model"]["display_name"].as_str().unwrap_or("");

    let cwd = tildify(raw_cwd, home);

    let mut segs: Vec<String> = Vec::new();
    if !model.is_empty() {
        segs.push(format!("{GREY}{model}{RESET}"));
    }
    segs.push(format!("{BLUE}{cwd}{RESET}"));

    if let Some(repo) = ws.get("repo").filter(|r| !r.is_null()) {
        let owner = repo["owner"].as_str().unwrap_or("");
        let name = repo["name"].as_str().unwrap_or("");
        segs.push(format!("{GREY}{owner}/{name}{RESET}"));
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
    let _ = io::stdout().write_all(render(&v, &home).as_bytes());
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
    fn render_cases() {
        let cases = [
            (
                "renders the full line",
                json!({
                    "workspace": {
                        "current_dir": "/home/u/proj",
                        "repo": { "owner": "anthropics", "name": "claude-code" }
                    },
                    "model": { "display_name": "Opus 4.8 (1M context)" },
                    "context_window": { "total_input_tokens": 44_000, "context_window_size": 1_000_000 }
                }),
                "/home/u",
                format!(
                    "{GREY}Opus 4.8 (1M context){RESET} \
                     {BLUE}~/proj{RESET} \
                     {GREY}anthropics/claude-code{RESET} \
                     {YELLOW}44k/1M (4%){RESET}"
                ),
            ),
            (
                "omits absent segments and keeps cwd",
                json!({ "cwd": "/tmp/scratch" }),
                "/home/u",
                format!("{BLUE}/tmp/scratch{RESET}"),
            ),
            (
                "skips context when size is zero",
                json!({
                    "cwd": "/tmp",
                    "context_window": { "total_input_tokens": 10, "context_window_size": 0 }
                }),
                "/home/u",
                format!("{BLUE}/tmp{RESET}"),
            ),
            (
                "leaves cwd outside home untouched",
                json!({ "cwd": "/var/data" }),
                "/home/u",
                format!("{BLUE}/var/data{RESET}"),
            ),
            (
                "abbreviates home itself",
                json!({ "cwd": "/home/u" }),
                "/home/u",
                format!("{BLUE}~{RESET}"),
            ),
            (
                "abbreviates only at a path boundary",
                json!({ "cwd": "/home/username" }),
                "/home/u",
                format!("{BLUE}/home/username{RESET}"),
            ),
            (
                "tolerates a trailing slash in home",
                json!({ "cwd": "/home/u/proj" }),
                "/home/u/",
                format!("{BLUE}~/proj{RESET}"),
            ),
            (
                "does not abbreviate when home is empty",
                json!({ "cwd": "/home/u/proj" }),
                "",
                format!("{BLUE}/home/u/proj{RESET}"),
            ),
        ];
        for (name, v, home, expected) in cases {
            assert_eq!(render(&v, home), expected, "{name}");
        }
    }
}
