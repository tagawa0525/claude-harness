//! block-no-verify: git の hook を飛ばす `git commit` と `git merge` を止める
//!
//! 機密情報の検査（gitleaks）、main への直接コミットのガード、形式の検査は git の pre-commit と commit-msg に、
//! ローカルのマージの検査は pre-merge-commit にある（ADR-0015）。次はそれを飛ばすので、コマンドを実行する前の
//! PreToolUse で止める:
//! - `--no-verify` とその省略形（git は一意な省略形を受け付ける）。commit では `-n` も
//! - `git -c core.hooksPath=…`（hook の置き場所を変える）
//!
//! エスケープ: コマンドに `ALLOW_NO_VERIFY=1` を付ける。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct BlockNoVerify;

/// `--no-verify` か、その省略形（`--no-v` 以上）か
fn is_no_verify(text: &str) -> bool {
    text.len() >= "--no-v".len() && "--no-verify".starts_with(text)
}

/// commit の短縮形のまとまり（`-anm` など）に `-n` があるか。値を取るオプション（`-m msg` を `-mmsg` と
/// 続けて書く形を含む）より後は値なので見ない
fn commit_cluster_has_n(arg: &Arg) -> bool {
    let t = &arg.text;
    if !t.starts_with('-') || t.starts_with("--") {
        return false;
    }
    for ch in t[1..].chars() {
        match ch {
            'n' => return true,
            // 値を取る（S と u は値を続けて書くときだけ取る）
            'm' | 'F' | 'C' | 'c' | 't' | 'S' | 'u' => return false,
            _ => {}
        }
    }
    false
}

/// サブコマンドより前の `-c core.hooksPath=…`（設定のキーは大文字と小文字を問わない）
fn overrides_hooks_path(args: &[Arg]) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = if a.text == "-c" {
            it.next().map(|v| v.text.as_str())
        } else {
            a.text.strip_prefix("-c")
        };
        if value.is_some_and(|v| v.to_ascii_lowercase().starts_with("core.hookspath=")) {
            return true;
        }
        if !a.text.starts_with('-') {
            return false;
        }
    }
    false
}

impl Rule for BlockNoVerify {
    fn name(&self) -> &'static str {
        "block-no-verify"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        if shell.has_escape("ALLOW_NO_VERIFY") {
            return Vec::new();
        }
        for cmd in shell.commands() {
            let Some((sub, args)) = cmd.git_subcommand() else {
                continue;
            };
            if sub != "commit" && sub != "merge" {
                continue;
            }
            let options = args.iter().take_while(|a| a.text != "--");
            let skips = overrides_hooks_path(&cmd.args)
                || options
                    .into_iter()
                    .any(|a| is_no_verify(&a.text) || (sub == "commit" && commit_cluster_has_n(a)));
            if skips {
                return vec![Finding::Deny(format!(
                    "git {sub} の --no-verify（commit の -n、省略形を含む）と git -c core.hooksPath= は、git の hook の検査（機密情報、main への直接コミット、形式、ローカルのマージ）を飛ばすので止めます（ADR-0015）。\nhook が止めた理由を直してから実行してください。どうしても必要な場合は ALLOW_NO_VERIFY=1 をコマンドに付けて実行してください。"
                ))];
            }
        }
        Vec::new()
    }
}
