//! deny の記録（ADR-0090）。状態ディレクトリの `claude-harness/deny.jsonl` に 1 件 1 行の JSON を追記する。
//! 記録は補助なので、失敗は無視し、判定も出力も変えない

use crate::input::Input;
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// コマンドと理由を残す文字数。コマンドには秘密が含まれうるので、全文は残さない
const MAX_CHARS: usize = 200;

/// `CLAUDE_HARNESS_DENY_LOG=0` で無効にする
fn enabled() -> bool {
    std::env::var_os("CLAUDE_HARNESS_DENY_LOG").is_none_or(|v| v != "0")
}

fn state_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".local/state"))
        });
    base.map(|b| b.join("claude-harness"))
}

fn head(s: &str) -> String {
    s.chars().take(MAX_CHARS).collect()
}

/// UNIX 秒を `YYYY-MM-DDTHH:MM:SSZ` にする。日付は Howard Hinnant の civil_from_days
/// （https://howardhinnant.github.io/date_algorithms.html）。時刻のためだけに chrono を足さない
fn iso8601_utc(secs: u64) -> String {
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn append(line: &str) -> std::io::Result<()> {
    let dir = state_dir().ok_or_else(|| std::io::Error::other("no state dir"))?;
    std::fs::create_dir_all(&dir)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    // 1 回の write で書く。並行する hook の行が混ざらない
    opts.open(dir.join("deny.jsonl"))?
        .write_all(line.as_bytes())
}

/// ルール `rule` の Deny（`reason` はルール名の接頭辞の前の理由）を記録する
pub fn record(rule: &str, input: &Input, reason: &str) {
    if !enabled() {
        return;
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut v = json!({
        "time": iso8601_utc(secs),
        "rule": rule,
        "cwd": input.cwd.to_string_lossy(),
    });
    if !input.session_id.is_empty() {
        v["session_id"] = json!(input.session_id);
    }
    v["command"] = json!(head(&input.command));
    v["reason"] = json!(head(reason));
    let _ = append(&format!("{v}\n"));
}

#[cfg(test)]
mod tests {
    use super::iso8601_utc;

    #[test]
    fn formats_known_instants() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_794_787_200 + 3661), "2026-11-16T01:01:01Z");
    }
}
