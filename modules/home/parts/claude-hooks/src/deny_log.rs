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
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        // mode は新規作成のときだけ効くので、既存のディレクトリの権限も締める
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(&dir)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut file = opts.open(dir.join("deny.jsonl"))?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    // ロックはかけない。ロックを持ったプロセスが止まると、PreToolUse の hook が無期限に待って固まる（ログは判定も
    // 速度も変えない、ADR-0090）。O_APPEND の write 1 回は、書き込み位置の決定と書き込みが分かれないので、並行する
    // hook の行は混ざらない。行全体を 1 つのバッファで 1 回だけ write し、PIPE_BUF 以下に収める（write_all は
    // 途中で分けて複数回の write になりうるので使わない）
    let n = file.write(line.as_bytes())?;
    if n == line.len() {
        Ok(())
    } else {
        Err(std::io::Error::other("short write"))
    }
}

/// 1 行の長さの上限（バイト）。POSIX の PIPE_BUF の最小値。これ以下なら 1 回の write で書ききる
const MAX_LINE_BYTES: usize = 4096;

/// 記録する 1 行（改行付き）。文字数の上限で切った後でも JSON のエスケープで上限を超えるときは、
/// 収まるまで command、reason、cwd、session_id の文字数の上限を半分にする
fn build_line(
    rule: &str,
    secs: u64,
    cwd: &str,
    session_id: &str,
    command: &str,
    reason: &str,
) -> String {
    let mut limit = MAX_CHARS;
    loop {
        let cut = |s: &str| -> String { s.chars().take(limit).collect() };
        let mut v = json!({
            "time": iso8601_utc(secs),
            "rule": rule,
            "cwd": cut(cwd),
        });
        if !session_id.is_empty() {
            v["session_id"] = json!(cut(session_id));
        }
        v["command"] = json!(cut(command));
        v["reason"] = json!(cut(reason));
        let line = format!("{v}\n");
        if line.len() <= MAX_LINE_BYTES || limit == 0 {
            return line;
        }
        limit /= 2;
    }
}

/// ルール `rule` の Deny（`reason` はルール名の接頭辞の前の理由）を記録する
pub fn record(rule: &str, input: &Input, reason: &str) {
    if !enabled() {
        return;
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = build_line(
        rule,
        secs,
        &input.cwd.to_string_lossy(),
        &input.session_id,
        &input.command,
        reason,
    );
    let _ = append(&line);
}

#[cfg(test)]
mod tests {
    use super::{MAX_LINE_BYTES, build_line, iso8601_utc};

    #[test]
    fn a_line_stays_within_one_atomic_write() {
        let nasty = "\u{1}".repeat(5000);
        let line = build_line("r", 0, &"d/".repeat(3000), &nasty, &nasty, &nasty);
        assert!(line.len() <= MAX_LINE_BYTES, "{}", line.len());
        assert!(line.ends_with('\n'));
        serde_json::from_str::<serde_json::Value>(&line).unwrap();
    }

    #[test]
    fn formats_known_instants() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_794_787_200 + 3661), "2026-11-16T01:01:01Z");
    }
}
