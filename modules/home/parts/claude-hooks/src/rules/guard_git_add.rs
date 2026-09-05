//! guard-git-add: 対象を絞らない git add を止める
//!
//! `git add -A` / `--all` / `.` / `:/` / `*` は未追跡ファイル（.env や鍵など）をまとめて
//! ステージするため禁止する。block-secret-commit（内容の検査）と対で効く。
//! 1 コミット 1 論理変更のためにも、`git add <file>` / `git add -p` で対象を選ぶ。
//!
//! 通すもの: パスを指定した add（範囲を限定した `git add -A src/` も可）、`-u`（追跡済みのみ）、
//! `-p` / `-i`（対話的に選ぶ）。エスケープはコマンドに `ALLOW_GIT_ADD_ALL=1` を付ける。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell, short_cluster_has};

pub struct GuardGitAdd;

fn deny(why: &str) -> Finding {
    Finding::Deny(format!(
        "{why}（未追跡ファイルをまとめてステージし、機密情報の混入と無関係な変更の混在を招くため）。\ngit status で対象を確認し、git add <file> または git add -p で論理単位ごとにステージしてください。\nどうしても必要な場合は ALLOW_GIT_ADD_ALL=1 をコマンドに付けて実行してください。"
    ))
}

/// `git add` の引数を検査し、止める理由があれば返す
fn check_add(args: &[Arg]) -> Option<String> {
    let mut all = false;
    let mut pathspecs: Vec<&str> = Vec::new();
    let mut after_dashdash = false;
    for a in args {
        let t = a.text.as_str();
        if after_dashdash {
            pathspecs.push(t);
        } else if t == "--" {
            after_dashdash = true;
        } else if t == "--all" || t == "--no-ignore-removal" {
            all = true;
        } else if t.starts_with("--") {
        } else if short_cluster_has(a, 'A') {
            all = true;
        } else if t.starts_with('-') {
        } else {
            pathspecs.push(t);
        }
    }
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

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        if shell.has_escape("ALLOW_GIT_ADD_ALL") {
            return Vec::new();
        }
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
            if let Some(why) = check_add(args) {
                return vec![deny(&why)];
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
        let cs = Shell::parse(src).commands();
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
}
