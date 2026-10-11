//! block-no-verify: git の hook の検査を飛ばすコマンドを止める
//!
//! 機密情報の検査（gitleaks）、main への直接コミットのガード、形式の検査は git の pre-commit と commit-msg に、
//! ローカルのマージの検査と機密情報の検査は pre-merge-commit にある（ADR-0015）。次はそれを飛ばすので、
//! コマンドを実行する前の PreToolUse で止める:
//! - `git commit` / `merge` / `pull` の `--no-verify` とその省略形（git は一意な省略形を受け付ける）。commit の `-n`
//! - hook の置き場所を変える: `git -c core.hooksPath=…`、`--config-env=core.hooksPath=…`、`git config core.hooksPath
//!   …`、`GIT_CONFIG_*` の環境変数
//! - hook が Claude Code のセッションと分かる `CLAUDECODE=1` を、別の値にする・外す（`env -u`、`unset`）
//!
//! エスケープ: コマンドに `ALLOW_NO_VERIFY=1` を付ける。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell, git_global_takes_value};

pub struct BlockNoVerify;

/// `--no-verify` か、その省略形（`--no-v` 以上）か
fn is_no_verify(text: &str) -> bool {
    text.len() >= "--no-v".len() && "--no-verify".starts_with(text)
}

/// commit の短縮形のまとまり（`-anm` など）に `-n` があるか。値を取るオプション（`-m msg` を `-mmsg` と
/// 続けて書く形を含む）より後は値なので見ない
fn commit_cluster_has_n(text: &str) -> bool {
    if !text.starts_with('-') || text.starts_with("--") {
        return false;
    }
    for ch in text[1..].chars() {
        match ch {
            'n' => return true,
            // 値を取る（S と u は値を続けて書くときだけ取る）
            'm' | 'F' | 'C' | 'c' | 't' | 'S' | 'u' => return false,
            _ => {}
        }
    }
    false
}

/// commit のオプションのうち、次の引数を値として取るもの（値が `-n` で始まっても `-n` ではない）
fn commit_takes_value(text: &str) -> bool {
    matches!(
        text,
        "-m" | "-F"
            | "-C"
            | "-c"
            | "-t"
            | "--message"
            | "--file"
            | "--reuse-message"
            | "--reedit-message"
            | "--author"
            | "--date"
            | "--template"
            | "--fixup"
            | "--squash"
            | "--cleanup"
            | "--trailer"
            | "--pathspec-from-file"
    ) || (text.starts_with('-')
        && !text.starts_with("--")
        && text.len() > 2
        && matches!(text.chars().last(), Some('m' | 'F' | 'C' | 'c' | 't'))
        && !text[1..text.len() - 1].contains(['m', 'F', 'C', 'c', 't', 'S', 'u']))
}

/// commit の引数に `-n`（`--no-verify` の短縮形）があるか。値の引数は見ない
fn commit_has_n(args: &[Arg]) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a.text == "--" {
            return false;
        }
        if commit_cluster_has_n(&a.text) {
            return true;
        }
        if commit_takes_value(&a.text) {
            it.next();
        }
    }
    false
}

fn is_hooks_path(setting: &str) -> bool {
    setting.to_ascii_lowercase().starts_with("core.hookspath=")
}

/// サブコマンドより前の `-c core.hooksPath=…` と `--config-env=core.hooksPath=…`（キーは大文字と小文字を問わない）
fn overrides_hooks_path(args: &[Arg]) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !a.text.starts_with('-') {
            // サブコマンドに着いた
            return false;
        }
        let value = if a.text == "-c" || a.text == "--config-env" {
            it.next().map(|v| v.text.as_str())
        } else if git_global_takes_value(&a.text) {
            // -C dir などの値は読み飛ばす
            it.next();
            None
        } else if let Some(v) = a.text.strip_prefix("--config-env=") {
            Some(v)
        } else {
            a.text.strip_prefix("-c")
        };
        if value.is_some_and(is_hooks_path) {
            return true;
        }
    }
    false
}

/// `git config` が hook の置き場所（core.hooksPath）を変えるか。書く・消す（`--unset` と、新しい書き方の
/// `set` / `unset`）と、core の節の削除と名前の変更（`--remove-section` / `--rename-section` と `remove-section` /
/// `rename-section`）を止める。読むだけ（`--get` / `get`）は通す
fn sets_hooks_path(args: &[Arg]) -> bool {
    let texts: Vec<&str> = args.iter().map(|a| a.text.as_str()).collect();
    let section_action = texts.iter().position(|t| {
        matches!(
            *t,
            "--remove-section" | "--rename-section" | "remove-section" | "rename-section"
        )
    });
    if let Some(i) = section_action {
        return texts[i + 1..]
            .iter()
            .find(|t| !t.starts_with('-'))
            .is_some_and(|t| t.eq_ignore_ascii_case("core"));
    }
    let Some(key) = texts
        .iter()
        .position(|t| t.eq_ignore_ascii_case("core.hookspath"))
    else {
        return false;
    };
    let before = &texts[..key];
    if before.iter().any(|t| t.starts_with("--get") || *t == "get") {
        return false;
    }
    before
        .iter()
        .any(|t| t.starts_with("--unset") || *t == "unset" || *t == "set")
        || texts.get(key + 1).is_some_and(|v| !v.starts_with('-'))
}

/// hook の置き場所を環境変数で変えるか、CLAUDECODE を 1 以外にする・外すか
fn changes_environment(shell: &Shell) -> bool {
    let assigned = shell.assignments().into_iter().any(|(name, value)| {
        name.starts_with("GIT_CONFIG_") || (name == "CLAUDECODE" && value != "1")
    });
    assigned
        || shell.unsets("CLAUDECODE")
        || shell.commands().iter().any(|c| match c.name.as_str() {
            "env" => {
                c.args
                    .windows(2)
                    .any(|w| w[0].text == "-u" && w[1].text == "CLAUDECODE")
                    || c.args
                        .iter()
                        .any(|a| a.text == "-uCLAUDECODE" || a.text == "--unset=CLAUDECODE")
            }
            _ => false,
        })
}

impl Rule for BlockNoVerify {
    fn name(&self) -> &'static str {
        "block-no-verify"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        if shell.has_escape("ALLOW_NO_VERIFY") {
            return Vec::new();
        }
        let mut skips = changes_environment(shell);
        for cmd in shell.commands() {
            let Some((sub, args)) = cmd.git_subcommand() else {
                continue;
            };
            skips |= match sub {
                "config" => sets_hooks_path(args),
                "commit" | "merge" | "pull" => {
                    overrides_hooks_path(&cmd.args)
                        || args
                            .iter()
                            .take_while(|a| a.text != "--")
                            .any(|a| is_no_verify(&a.text))
                        || (sub == "commit" && commit_has_n(args))
                }
                _ => false,
            };
        }
        if skips {
            return vec![Finding::Deny(
                "git の hook の検査（機密情報、main への直接コミット、形式、ローカルのマージ）を飛ばすので止めます（ADR-0015）: --no-verify（commit の -n、省略形を含む）、hook の置き場所の変更（-c / --config-env / git config の core.hooksPath、GIT_CONFIG_*）、CLAUDECODE の書き換え。\nhook が止めた理由を直してから実行してください。どうしても必要な場合は ALLOW_NO_VERIFY=1 をコマンドに付けて実行してください。".to_string(),
            )];
        }
        Vec::new()
    }
}
