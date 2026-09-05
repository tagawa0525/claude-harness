//! block-secret-commit: 機密情報を含む変更の git commit を gitleaks で止める
//!
//! 検出そのものは自前で持たず、既存の定番ツール gitleaks に委ねる（ルール集とエントロピー判定、
//! `gitleaks:allow` / .gitleaksignore / .gitleaks.toml による除外は gitleaks の仕組みをそのまま使う）。
//! この hook が担うのは次の 2 つだけ:
//! - いつ何を渡して呼ぶか: `git commit` の対象ディレクトリで `gitleaks git --staged`。
//!   -a / --all なら未ステージの追跡ファイルの変更も対象なので `--pre-commit` でも走らせる
//!   （gitleaks は両方のフラグを同時に指定すると staged だけを見る）
//! - 結果の扱い: 漏えい（exit 1 + JSON）は場所とルール ID だけを理由に出す（値は出さない）。
//!   gitleaks が無い・失敗した場合は「検査できない」として deny する（黙って通さない）
//!
//! エスケープ: コマンドに `ALLOW_SECRET_COMMIT=1` を付ける。
//! gitleaks は claude-code.nix の home.packages で配布する

use std::path::Path;
use std::process::Command;

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Shell, short_cluster_has};

pub struct BlockSecretCommit;

enum Scan {
    Clean,
    /// `path:line (rule-id)` の一覧
    Leaks(Vec<String>),
    /// gitleaks が無い、または実行に失敗した
    Unavailable(String),
}

/// `gitleaks git <mode> …` を dir で実行して結果を分類する
fn run_gitleaks(dir: &Path, mode: &str) -> Scan {
    let out = Command::new("gitleaks")
        .args([
            "git",
            mode,
            "--no-banner",
            "--redact",
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
        Err(e) => return Scan::Unavailable(format!("gitleaks を起動できません: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let findings: Option<Vec<serde_json::Value>> = serde_json::from_str(stdout.trim()).ok();
    match (out.status.code(), findings) {
        (Some(0), _) => Scan::Clean,
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
        (code, _) => Scan::Unavailable(format!(
            "gitleaks が失敗しました（exit {}）: {}",
            code.map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into()),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

impl Rule for BlockSecretCommit {
    fn name(&self) -> &'static str {
        "block-secret-commit"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if shell.has_escape("ALLOW_SECRET_COMMIT") {
            return Vec::new();
        }
        for cmd in shell.commands() {
            let Some(("commit", args)) = cmd.git_subcommand() else {
                continue;
            };
            let dir = shell.target_dir(&cmd, &input.cwd);
            let all = args
                .iter()
                .any(|a| a.text == "--all" || short_cluster_has(a, 'a'));
            let modes: &[&str] = if all {
                &["--staged", "--pre-commit"]
            } else {
                &["--staged"]
            };
            let mut leaks = Vec::new();
            for mode in modes {
                match run_gitleaks(&dir, mode) {
                    Scan::Clean => {}
                    Scan::Leaks(list) => leaks.extend(list),
                    Scan::Unavailable(why) => {
                        return vec![Finding::Deny(format!(
                            "機密情報の検査ができないためコミットを止めます（{why}）。\ngitleaks は claude-code.nix の home.packages で配布しています。rebuild で反映するか、どうしても必要な場合は ALLOW_SECRET_COMMIT=1 をコマンドに付けて実行してください。"
                        ))];
                    }
                }
            }
            if leaks.is_empty() {
                continue;
            }
            let list: Vec<String> = leaks.iter().map(|f| format!("- {f}")).collect();
            return vec![Finding::Deny(format!(
                "機密情報らしき内容がコミット対象に含まれています（gitleaks）:\n{}\n\n対処: 該当ファイルを git restore --staged <file> で外し、必要なら .gitignore に追加してください。\n既に push 済みの値はローテーションしてください。\n誤検出なら、その行に gitleaks:allow を書く（.gitleaksignore / .gitleaks.toml でも可）か、コマンドに ALLOW_SECRET_COMMIT=1 を付けて実行してください。",
                list.join("\n")
            ))];
        }
        Vec::new()
    }
}
