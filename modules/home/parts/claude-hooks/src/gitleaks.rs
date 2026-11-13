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
    /// gitleaks が失敗した（直下を求められなかったときを含む）
    Failed(Failure),
}

pub struct Failure {
    /// 何が失敗したか（`gitleaks が失敗しました（exit 1）` など）
    pub what: String,
    /// gitleaks か git の標準エラー
    pub errors: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.errors.is_empty() {
            write!(f, "{}", self.what)
        } else {
            write!(f, "{}: {}", self.what, self.errors)
        }
    }
}

/// dir のリポジトリの直下で `<bin> git <mode> …` を実行して結果を分類する。mode は `--staged`（index と HEAD の差）か
/// `--pre-commit`（作業ツリーと HEAD の差）。gitleaks は `.` を作業ディレクトリからの相対で読み、その下の変更と
/// 設定（.gitleaks.toml、.gitleaksignore）だけを見るので、サブディレクトリからでも直下で調べる
pub fn scan(bin: &str, dir: &Path, mode: &str) -> Scan {
    // git の理由（リポジトリの外、safe.directory など）を見せるので、git::git でなく標準エラーを受ける git::run で呼ぶ
    let top = match crate::git::run(dir, &["rev-parse", "--show-toplevel"], None) {
        Some(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string(),
        out => {
            return Scan::Failed(Failure {
                what: format!("{} のリポジトリの直下を求められません", dir.display()),
                errors: out
                    .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
                    .unwrap_or_default(),
            });
        }
    };
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
        .current_dir(&top)
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return Scan::Missing(e),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let findings: Option<Vec<serde_json::Value>> = serde_json::from_str(stdout.trim()).ok();
    // --staged（index と HEAD の差）の標準エラーは、中の git diff の失敗なので検査できなかったとみなす。--pre-commit
    // （作業ツリーとの差）は、git diff の警告（`* text=auto` の下の CRLF のファイルの「CRLF will be replaced by LF」）
    // も ERR [git] warning: として出して 0 を返す（gitleaks 8.30.1 で確かめた）ので、終了コードだけで決める
    let errors_mean_failure = mode == "--staged";
    match (out.status.code(), findings) {
        (Some(0), _) if !errors_mean_failure || stderr.trim().is_empty() => Scan::Clean,
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
        (code, _) => Scan::Failed(Failure {
            what: format!(
                "gitleaks が失敗しました（exit {}）",
                code.map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into())
            ),
            errors: stderr.trim().to_string(),
        }),
    }
}
