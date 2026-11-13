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
    let errors = stderr_errors(&stderr);
    match (out.status.code(), findings) {
        (Some(0), _) if errors.is_empty() => Scan::Clean,
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
            what: match code {
                Some(0) => {
                    "gitleaks が失敗しました（終了コードは 0 だが、標準エラーに行を出して差分を読むのをやめた。git の警告でも止まるので、警告の原因を直すと検査できる）".to_string()
                }
                Some(c) => format!("gitleaks が失敗しました（exit {c}）"),
                None => "gitleaks が失敗しました（signal）".to_string(),
            },
            errors: strip_colors(stderr.trim()),
        }),
    }
}

/// 標準エラーの行（色を外したもの）。gitleaks は、中の git diff が標準エラーに何か出すと、警告でも（`* text=auto` の
/// 下の CRLF のファイルの「CRLF will be replaced by LF」など）ERR の行と stderr is not empty を出し、差分を読むのを
/// やめて 0 と空の一覧を返す（gitleaks 8.30.1 で、同じトークンが LF の差分では見つかり、CRLF の警告の出る差分では
/// 見つからないことを確かめた）。どの行も、検査できなかったしるしとして扱う
fn stderr_errors(stderr: &str) -> Vec<String> {
    strip_colors(stderr)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// 端末の色の符号（ESC [ … m）を外す
fn strip_colors(text: &str) -> String {
    static COLOR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new("\x1b\\[[0-9;]*m").expect("正規表現は正しい")
    });
    COLOR.replace_all(text, "").into_owned()
}
