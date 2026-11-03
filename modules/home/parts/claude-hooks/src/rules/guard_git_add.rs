//! guard-git-add: 対象を絞らない git add を止める
//!
//! `git add -A` / `--all` / `.` / `:/` / `*` は未追跡ファイル（.env や鍵など）をまとめて
//! ステージするため禁止する。git の pre-commit の機密情報の検査（内容の検査。ADR-0015）と対で効く。
//! 1 コミット 1 論理変更のためにも、`git add <file>` / `git add -p` で対象を選ぶ。
//!
//! 通すもの: パスを指定した add（範囲を限定した `git add -A src/` も可）、`-u`（追跡済みのみ）、
//! `-p` / `-i`（対話的に選ぶ）。セッションの scratchpad の下に git init で作った一時のリポジトリ（scratchpad_repo.rs）
//! では止めない。エスケープはコマンドに `ALLOW_GIT_ADD_ALL=<理由>` を付ける。

use super::{Rule, escape_hint, scratchpad_repo};
use crate::git_args::{git_flag, positionals};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct GuardGitAdd;

fn deny(why: &str, not_temp: &str) -> Finding {
    Finding::Deny(format!(
        "{why}（未追跡ファイルをまとめてステージし、機密情報の混入と無関係な変更の混在を招くため）。\ngit status で対象を確認し、git add <file> または git add -p で論理単位ごとにステージしてください。\n{}（このコマンドは当たりません: {not_temp}）。\n{}",
        scratchpad_repo::EXCEPTION,
        escape_hint("ALLOW_GIT_ADD_ALL")
    ))
}

/// `git add` の引数を検査し、止める理由があれば返す
fn check_add(args: &[Arg]) -> Option<String> {
    let all = git_flag("add", args, Some('A'), &["--all", "--no-ignore-removal"]);
    let pathspecs: Vec<&str> = positionals("add", args)
        .into_iter()
        .map(|a| a.text.as_str())
        .collect();
    // --pathspec-from-file のファイルの中身（`.` や `:/` も書ける）は見ないので、パスを指定したことにしない
    if all && pathspecs.is_empty() {
        return Some("git add -A / --all はパスを指定しない限り禁止です".to_string());
    }
    pathspecs
        .iter()
        .find(|p| matches!(**p, "." | "./" | ":/" | "*"))
        .map(|p| format!("git add {p} は禁止です"))
}

impl Rule for GuardGitAdd {
    fn name(&self) -> &'static str {
        "guard-git-add"
    }

    fn escape(&self) -> Option<&'static str> {
        Some("ALLOW_GIT_ADD_ALL")
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some(("add", args)) = cmd.git_subcommand() else {
                continue;
            };
            if cmd.has_error {
                let raw: Vec<&str> = args.iter().map(|a| a.raw.as_str()).collect();
                return vec![Finding::Deny(format!(
                    "git add の引数を解析できません（クォートが不整合）: {}",
                    raw.join(" ")
                ))];
            }
            if let Some(why) = check_add(args)
                && let Err(not_temp) = scratchpad_repo::check(input, shell, cmd)
            {
                return vec![deny(&why, &not_temp)];
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::check_add;
    use crate::shell::Shell;

    fn args(src: &str) -> Option<String> {
        let cs = Shell::parse(src).commands().to_vec();
        let (_, a) = cs[0].git_subcommand().unwrap();
        check_add(a)
    }

    #[test]
    fn scoped_and_tracked_only_are_allowed() {
        assert!(args("git add -A src/").is_none());
        assert!(args("git add --all -- src/").is_none());
        assert!(args("git add -u").is_none());
        assert!(args("git add ./src").is_none());
        assert!(args("git add .claude/").is_none());
    }

    #[test]
    fn unscoped_forms_are_denied() {
        for c in [
            "git add -A",
            "git add --all",
            "git add .",
            "git add :/",
            "git add *",
            "git add -An",
        ] {
            assert!(args(c).is_some(), "{c}");
        }
    }

    mod scratchpad {
        use super::super::GuardGitAdd;
        use crate::output::Finding;
        use crate::rules::Rule;
        use crate::rules::scratchpad_repo::fixture::{Fixture, SID, input};
        use crate::shell::Shell;
        use std::path::Path;

        fn deny_reason(src: &str, cwd: &Path) -> Option<String> {
            let findings = GuardGitAdd.check(&input(cwd, SID), &Shell::parse(src));
            findings.into_iter().find_map(|f| match f {
                Finding::Deny(r) => Some(r),
                _ => None,
            })
        }

        #[test]
        fn unscoped_add_in_a_temp_repo_under_scratchpad_is_allowed() {
            let f = Fixture::new("add-allow");
            let repo = f.temp_repo("r");
            for src in [
                "git add -A".to_string(),
                "git add .".to_string(),
                format!("cd {} && git add -A", repo.display()),
                format!("git -C {} add .", repo.display()),
            ] {
                assert_eq!(deny_reason(&src, &repo), None, "{src}");
            }
        }

        #[test]
        fn unscoped_add_outside_scratchpad_is_denied_with_why_the_exception_does_not_apply() {
            let f = Fixture::new("add-deny");
            let why = deny_reason("git add -A", &f.prod()).expect("deny");
            assert!(why.contains("git add -p"), "{why}");
            assert!(why.contains("scratchpad の外"), "{why}");
        }
    }
}
