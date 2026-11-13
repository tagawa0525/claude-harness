//! gitleaks でコミットに入る中身を検査する（ADR-0015）。PreToolUse の block-secret-commit と、グローバルの git hook の
//! scan-secrets（crate::git_hook）が使う。検出そのものと除外（`gitleaks:allow`、.gitleaksignore、.gitleaks.toml）は
//! gitleaks に委ね、値は出させない（--redact）

use std::path::Path;
use std::process::Command;

pub enum Scan {
    Clean,
    /// `path:line (rule-id)` の一覧
    Leaks(Vec<String>),
    /// gitleaks を起動できない
    Missing(std::io::Error),
    /// gitleaks が失敗した。理由
    Failed(String),
}

/// `<bin> git <mode> …` を dir で実行して結果を分類する。mode は `--staged`（index と HEAD の差）か
/// `--pre-commit`（作業ツリーと HEAD の差）
pub fn scan(bin: &str, dir: &Path, mode: &str) -> Scan {
    // --log-level error: gitleaks は中の git diff の失敗を ERR に出すだけで、終了コード 0（漏えいなし）を返すので、
    // エラーの行だけを出させ、出たら検査できなかったとみなす
    let out = Command::new(bin)
        .args([
            "git",
            mode,
            "--no-banner",
            "--redact",
            "--log-level",
            "error",
            "--report-format",
            "json",
            "--report-path",
            "-",
            ".",
        ])
        .current_dir(dir)
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return Scan::Missing(e),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let findings: Option<Vec<serde_json::Value>> = serde_json::from_str(stdout.trim()).ok();
    match (out.status.code(), findings) {
        (Some(0), _) if stderr.trim().is_empty() => Scan::Clean,
        // 設定を読めないなどの失敗でも 1 を返すので、一覧が空でないときだけ漏えいとする
        (Some(1), Some(list)) if !list.is_empty() => Scan::Leaks(
            list.iter()
                .map(|f| {
                    format!(
                        "{}:{} ({})",
                        f["File"].as_str().unwrap_or("?"),
                        f["StartLine"].as_u64().unwrap_or(0),
                        f["RuleID"].as_str().unwrap_or("?")
                    )
                })
                .collect(),
        ),
        (code, _) => Scan::Failed(format!(
            "gitleaks が失敗しました（exit {}）: {}",
            code.map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into()),
            stderr.trim()
        )),
    }
}
