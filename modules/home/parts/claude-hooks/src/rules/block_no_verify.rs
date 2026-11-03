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
//! セッションの scratchpad の下に git init で作った一時のリポジトリ（scratchpad_repo.rs）への commit / merge / pull は
//! 止めない。`git config` は `--global` でも同じ形で書けるので、そこでも止める。
//!
//! エスケープ: コマンドに `ALLOW_NO_VERIFY=<理由>` を付ける。

use super::{Rule, escape_hint, scratchpad_repo};
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

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        // 止める理由と、scratchpad の一時のリポジトリの例外に当たらない理由（commit / merge / pull のときだけ）
        let mut skips: Option<Option<String>> = None;
        for cmd in shell.commands() {
            let Some((sub, args)) = cmd.git_subcommand() else {
                continue;
            };
            let skipping = match sub {
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
            if !skipping {
                continue;
            }
            // `git config` は `--global` でも同じ形で書けるので、scratchpad の下でも例外にしない
            let not_temp = if sub == "config" {
                None
            } else {
                match scratchpad_repo::check(input, shell, cmd) {
                    Ok(()) => continue,
                    Err(why) => Some(why),
                }
            };
            skips.get_or_insert(not_temp);
        }
        if let Some(not_temp) = skips {
            let exception = match not_temp {
                Some(why) => format!(
                    "\n{}（このコマンドは当たりません: {why}）。",
                    scratchpad_repo::EXCEPTION
                ),
                None => String::new(),
            };
            return vec![Finding::Deny(format!(
                "git の hook の検査（機密情報、main への直接コミット、形式、ローカルのマージ）を飛ばすので止めます: --no-verify（commit の -n、省略形を含む）、hook の置き場所の変更（-c / --config-env / git config の core.hooksPath、GIT_CONFIG_*）、CLAUDECODE の書き換え（-E の無い sudo と ssh の先には CLAUDECODE が渡りません）。{exception}\nhook が止めた理由を直してから実行してください。{}",
                escape_hint("ALLOW_NO_VERIFY")
            ))];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::BlockNoVerify;
    use crate::output::Finding;
    use crate::rules::Rule;
    use crate::rules::scratchpad_repo::fixture::{Fixture, SID, input};
    use crate::shell::Shell;
    use std::path::Path;

    fn deny_reason(src: &str, cwd: &Path) -> Option<String> {
        let findings = BlockNoVerify.check(&input(cwd, SID), &Shell::parse(src));
        findings.into_iter().find_map(|f| match f {
            Finding::Deny(r) => Some(r),
            _ => None,
        })
    }

    #[test]
    fn skipping_hooks_in_a_temp_repo_under_scratchpad_is_allowed() {
        let f = Fixture::new("nv-allow");
        let repo = f.temp_repo("r");
        let r = repo.display();
        for src in [
            "git -c core.hooksPath=/dev/null commit -qm x".to_string(),
            "git commit --no-verify -m x".to_string(),
            format!("cd {r} && git -c core.hooksPath=/dev/null commit -m x"),
            format!("git -C {r} -c core.hooksPath=/dev/null commit -m x"),
        ] {
            assert_eq!(deny_reason(&src, &repo), None, "{src}");
        }
    }

    #[test]
    fn skipping_hooks_outside_scratchpad_is_denied_with_why_the_exception_does_not_apply() {
        let f = Fixture::new("nv-deny");
        let why =
            deny_reason("git -c core.hooksPath=/dev/null commit -m x", &f.prod()).expect("deny");
        assert!(why.contains("scratchpad の外"), "{why}");
    }

    #[test]
    fn a_hook_skip_aimed_outside_is_denied_even_when_another_command_targets_scratchpad() {
        let f = Fixture::new("nv-mixed");
        let repo = f.temp_repo("r");
        let src = format!(
            "git -c core.hooksPath=/dev/null commit -m x && git -C {} commit --no-verify -m y",
            f.prod().display()
        );
        assert!(deny_reason(&src, &repo).is_some());
    }

    #[test]
    fn the_reason_outside_scratchpad_is_shown_whatever_the_order_of_the_commands() {
        let f = Fixture::new("nv-order");
        let repo = f.temp_repo("r");
        let prod = f.prod().display().to_string();
        for src in [
            format!("git config core.hooksPath /dev/null; git -C {prod} commit --no-verify -m x"),
            format!("git -C {prod} commit --no-verify -m x; git config core.hooksPath /dev/null"),
        ] {
            let why = deny_reason(&src, &repo).expect("deny");
            assert!(why.contains("scratchpad の外"), "{src}: {why}");
        }
    }

    #[test]
    fn git_config_of_hooks_path_is_denied_even_under_scratchpad() {
        // `git config --global` も同じ形で書けるので、scratchpad の下でも hook の置き場所の書き換えは例外にしない
        let f = Fixture::new("nv-config");
        let repo = f.temp_repo("r");
        assert!(deny_reason("git config core.hooksPath /dev/null", &repo).is_some());
    }
}
