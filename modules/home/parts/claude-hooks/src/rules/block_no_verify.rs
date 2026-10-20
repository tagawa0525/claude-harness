//! block-no-verify: git の hook の検査を飛ばすコマンドを止める
//!
//! 機密情報の検査（gitleaks）、main への直接コミットのガード、形式の検査は git の pre-commit と commit-msg に、
//! ローカルのマージの検査と機密情報の検査は pre-merge-commit にある（ADR-0015）。次はそれを飛ばすので、
//! コマンドを実行する前の PreToolUse で止める:
//! - `git commit` / `merge` / `pull` の `--no-verify` とその省略形（git は一意な省略形を受け付ける）。commit の `-n`
//! - hook の置き場所を変える: `git -c core.hooksPath=…`、`--config-env=core.hooksPath=…`、`git config core.hooksPath
//!   …`、`GIT_CONFIG_*` の環境変数
//! - hook が Claude Code のセッションと分かる `CLAUDECODE=1` を、別の値にする・外す（`env -u`、`env -i`、`unset`）
//!
//! env などで包んだコマンドは、shell.rs がほどいた中の git コマンドと、ラッパーが変える環境変数で見る。
//!
//! エスケープ: コマンドに `ALLOW_NO_VERIFY=<理由>` を付ける。

use super::{Rule, escape_hint};
use crate::git_args::{Item, items};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};
use crate::unwrap::{EnvChange, env_value};

pub struct BlockNoVerify;

/// `--no-verify` か、その省略形（`--no-v` 以上）か
fn is_no_verify(text: &str) -> bool {
    text.len() >= "--no-v".len() && "--no-verify".starts_with(text)
}

/// `-c` / `--config-env` の設定が core.hooksPath か（値の無い `core.hooksPath` は true として読まれる）
fn is_hooks_path(setting: &str) -> bool {
    let key = setting.split_once('=').map_or(setting, |(key, _)| key);
    key.eq_ignore_ascii_case("core.hookspath")
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

/// 環境変数が hook の置き場所を変えるもの（GIT_CONFIG_*）か
fn sets_hook_config(name: &str) -> bool {
    name.starts_with("GIT_CONFIG_")
}

/// シェルの代入と unset が、hook の置き場所を変えるか、hook が Claude Code のセッションと分かる
/// CLAUDECODE を 1 以外にする・外すか
fn changes_environment(shell: &Shell) -> bool {
    shell
        .assignments()
        .into_iter()
        .any(|(name, value)| sets_hook_config(&name) || (name == "CLAUDECODE" && value != "1"))
        || shell.unsets("CLAUDECODE")
}

impl Rule for BlockNoVerify {
    fn name(&self) -> &'static str {
        "block-no-verify"
    }

    fn escape(&self) -> Option<&'static str> {
        Some("ALLOW_NO_VERIFY")
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut skips = false;
        for cmd in shell.commands() {
            let Some((sub, args)) = cmd.git_subcommand() else {
                continue;
            };
            skips |= match sub {
                "config" => sets_hooks_path(args),
                // 環境変数（シェルの代入と unset、ラッパーの変更）は、hook が走るコマンドがあるときだけ見る
                // （`sudo ls` や `ssh host nvidia-smi`、`CLAUDECODE=0 echo ok` では hook は走らない）
                "commit" | "merge" | "pull" => {
                    changes_environment(shell)
                        || cmd.env.iter().any(|change| match change {
                            EnvChange::Set(name, _) | EnvChange::Unset(name) => {
                                sets_hook_config(name)
                            }
                            EnvChange::Clear => false,
                        })
                        || env_value(&cmd.env, "CLAUDECODE", Some("1")) != Some("1")
                        || cmd.git_config_overrides().iter().any(|c| is_hooks_path(c))
                        || items(sub, args).iter().any(|item| match item {
                            Item::Long(name) => is_no_verify(name),
                            Item::Short(ch) => sub == "commit" && *ch == 'n',
                            Item::Positional(_) => false,
                        })
                }
                _ => false,
            };
        }
        if skips {
            return vec![Finding::Deny(format!(
                "git の hook の検査（機密情報、main への直接コミット、形式、ローカルのマージ）を飛ばすので止めます: --no-verify（commit の -n、省略形を含む）、hook の置き場所の変更（-c / --config-env / git config の core.hooksPath、GIT_CONFIG_*）、CLAUDECODE の書き換え（-E の無い sudo と ssh の先には CLAUDECODE が渡りません）。\nhook が止めた理由を直してから実行してください。{}",
                escape_hint("ALLOW_NO_VERIFY")
            ))];
        }
        Vec::new()
    }
}
