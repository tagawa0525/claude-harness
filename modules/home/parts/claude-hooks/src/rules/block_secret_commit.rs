//! block-secret-commit: グローバルの git の hook を使わないリポジトリの git commit を、gitleaks で止める
//!
//! 機密情報の検査はグローバルの git の pre-commit と pre-merge-commit で行う（ADR-0015）。コミットの時点の
//! index を見るので、同じコマンドの git add も見落とさない。ただ、リポジトリがローカルの core.hooksPath
//! （husky など）でグローバルの hook を使わないときは、それが走らない。そのリポジトリでだけ、コマンドを
//! 実行する前のここで検査する（同じコマンドの git add は見えない）。
//!
//! 対象のディレクトリをコマンドの文字列から決められないとき（まだ無い、変数を含む）は、git の hook に任せて
//! 通す（#281。そのリポジトリがグローバルの hook を使うかも分からない）。
//!
//! 検出そのものは gitleaks に委ねる（`gitleaks:allow` / .gitleaksignore / .gitleaks.toml による除外も
//! gitleaks の仕組み）。漏えいは場所とルール ID だけを理由に出す。gitleaks が無い・失敗した場合は
//! 「検査できない」として deny する。
//!
//! エスケープ: コマンドに `ALLOW_SECRET_COMMIT=<理由>` を付ける。

use std::path::Path;

use super::{Rule, escape_hint};
use crate::git;
use crate::git_args::git_flag;
use crate::gitleaks::{self, Scan};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Escape, Shell};

pub struct BlockSecretCommit;

/// dir のリポジトリがグローバルの git の hook を使うか（実際に効く core.hooksPath がグローバルの値と同じか）
fn uses_global_hooks(dir: &Path) -> bool {
    let global = git::git(dir, &["config", "--global", "--get", "core.hooksPath"]);
    global.is_some() && git::git(dir, &["config", "--get", "core.hooksPath"]) == global
}

impl Rule for BlockSecretCommit {
    fn name(&self) -> &'static str {
        "block-secret-commit"
    }

    fn escape(&self) -> Option<&'static str> {
        Some("ALLOW_SECRET_COMMIT")
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some(("commit", args)) = cmd.git_subcommand() else {
                continue;
            };
            let dir = shell.target_dir(cmd, &input.cwd);
            // 変数やコマンド置換は展開しない（shell.rs）ので、含むパスは実物ではない。まだ無いディレクトリも
            // 同じく決められない。どちらも、そのリポジトリの git の hook に任せる
            let text = dir.to_string_lossy();
            if text.contains('$') || text.contains('`') || !dir.is_dir() || uses_global_hooks(&dir)
            {
                // git の hook は展開後の値しか見えないので、展開を含むエスケープはここで止める
                // （rules::run が理由を求める行を足す）
                if shell.escape("ALLOW_SECRET_COMMIT") == Escape::Expanded {
                    return vec![Finding::Deny(
                        "このリポジトリの機密情報の検査は git の pre-commit が行いますが、エスケープの値に展開（$VAR、$(…) など）が含まれ、git 側は展開後の値しか見えません。理由を字面で書いてください".to_string(),
                    )];
                }
                continue;
            }
            let all = git_flag("commit", args, Some('a'), &["--all"]);
            let modes: &[&str] = if all {
                &["--staged", "--pre-commit"]
            } else {
                &["--staged"]
            };
            let mut leaks = Vec::new();
            for mode in modes {
                // gitleaks は home.packages で PATH に置く（.claude/tests/hooks.sh は偽の gitleaks を PATH に置いて確かめる）
                let why = match gitleaks::scan("gitleaks", &dir, mode) {
                    Scan::Clean => continue,
                    Scan::Leaks(list) => {
                        leaks.extend(list);
                        continue;
                    }
                    Scan::Missing(e) => format!(
                        "gitleaks が PATH にありません（{e}）。gitleaks は claude-code.nix の home.packages で配布しています"
                    ),
                    Scan::Failed(failure) => failure.to_string(),
                };
                return vec![Finding::Deny(format!(
                    "機密情報の検査ができないためコミットを止めます（{why}）。\n{}",
                    escape_hint("ALLOW_SECRET_COMMIT")
                ))];
            }
            if leaks.is_empty() {
                continue;
            }
            let list: Vec<String> = leaks.iter().map(|f| format!("- {f}")).collect();
            return vec![Finding::Deny(format!(
                "機密情報らしき内容がコミット対象に含まれています（gitleaks）:\n{}\n\n対処: 該当ファイルを git restore --staged <file> で外し、必要なら .gitignore に追加してください。\n既に push 済みの値はローテーションしてください。\n誤検出なら、その行に gitleaks:allow を書いてください（.gitleaksignore / .gitleaks.toml でも可）。\n{}",
                list.join("\n"),
                escape_hint("ALLOW_SECRET_COMMIT")
            ))];
        }
        Vec::new()
    }
}
