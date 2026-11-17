//! guard-track-remote: リモート追跡ブランチを起点にしたブランチ作成で、`--no-track` を付けないものを止める
//!
//! `git switch -c <b> origin/main` などは、起点がリモート追跡ブランチだと上流を起点に設定する。
//! 引数なしの `git push` や `git pull` の行き先が origin/main になり、push 先が main になる。
//! `--track` / `-t` で上流を設定する意図を示した形は通す。
//!
//! 対象: `git switch -c|-C <b> <start>`、`git checkout -b|-B <b> <start>`、`git branch <b> <start>`。
//! 起点がリモート追跡ブランチ（`refs/remotes/<start>` がある）のときだけ止める。
//! `git worktree add` は worktree-add.sh を使う前提で対象外（そのスクリプトが `--no-track` を付ける。
//! guard-branch-base も起点つきの worktree add は見ない）。直接呼んだ `git worktree add -b <b> <path> origin/main` は止まらない。

use super::Rule;
use super::guard_branch_base::{is_branch_create_flags, read};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct GuardTrackRemote;

/// 上流を設定する意図が示されていない、起点つきのブランチ作成の（ブランチ名、起点）。
/// 起点を省いた作成は guard-branch-base の対象
fn tracking_start(sub: &str, args: &[Arg]) -> Option<(String, String)> {
    let (create, branch_mode): (&[&str], bool) = match sub {
        "switch" => (&["-c", "-C", "--create", "--force-create"], false),
        "checkout" => (&["-b", "-B"], false),
        "branch" => (&[], true),
        _ => return None,
    };
    let p = read(args, create, &[]);
    // 上流の扱いを明示した形（--track、-t、--no-track）は止めない
    let explicit = p.flags.iter().any(|f| {
        matches!(f.as_str(), "--track" | "--no-track")
            || f.starts_with("--track=")
            // まとめた短いフラグ（-t、-tf、-ft）
            || f.strip_prefix('-').is_some_and(|c| !c.starts_with('-') && c.contains('t'))
    });
    // switch / checkout の `--` の後はパス（ファイルを戻す形）。branch の `--` の後はブランチ名と起点
    let pathspec = !branch_mode
        && args
            .iter()
            .position(|a| a.text == "--")
            .is_some_and(|i| i + 1 < args.len());
    if explicit || pathspec {
        return None;
    }
    match (branch_mode, p.branch, p.positional.as_slice()) {
        (false, Some(name), [start]) => Some((name, start.clone())),
        (true, None, [name, start]) if is_branch_create_flags(&p.flags) => {
            Some((name.clone(), start.clone()))
        }
        _ => None,
    }
}

impl Rule for GuardTrackRemote {
    fn name(&self) -> &'static str {
        "guard-track-remote"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some((name, start)) = cmd
                .git_subcommand()
                .and_then(|(sub, args)| tracking_start(sub, args))
            else {
                continue;
            };
            let dir = shell.target_dir(cmd, &input.cwd);
            // origin/main、remotes/origin/main、refs/remotes/origin/main のどの書き方も完全な ref 名に解く
            let full = git::git(&dir, &["rev-parse", "--symbolic-full-name", &start]);
            if !full.is_some_and(|f| f.trim().starts_with("refs/remotes/")) {
                continue;
            }
            return vec![Finding::Deny(format!(
                "{start} を起点にしたブランチ {name} は、上流が {start} になります。引数なしの git push や git pull の\
                 行き先が {start} になり、push 先が既定ブランチになります。\n\
                 - --no-track を付ける: git switch --no-track -c {name} {start}（git checkout / git branch も同じ）\n\
                 - 上流を {start} にするのが意図なら --track を明示する"
            ))];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{TempDir, git, git_init};
    use super::*;

    fn parse(src: &str) -> Option<(String, String)> {
        let cs = Shell::parse(src).commands().to_vec();
        let (sub, args) = cs[0].git_subcommand()?;
        tracking_start(sub, args)
    }

    fn pair(b: &str, s: &str) -> Option<(String, String)> {
        Some((b.to_string(), s.to_string()))
    }

    #[test]
    fn start_given_without_track_option_is_a_target() {
        assert_eq!(
            parse("git switch -c feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(
            parse("git switch -C feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(
            parse("git switch -c feat/a --discard-changes origin/main"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(
            parse("git checkout -b feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(
            parse("git checkout -B feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(
            parse("git branch feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(
            parse("git branch -f feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
    }

    #[test]
    fn double_dash_is_a_pathspec_only_for_switch_and_checkout() {
        assert_eq!(
            parse("git branch -- feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(
            parse("git switch -c feat/a origin/main --"),
            pair("feat/a", "origin/main")
        );
        assert_eq!(parse("git checkout origin/main -- a.txt"), None);
        assert_eq!(parse("git checkout -b feat/a origin/main -- a.txt"), None);
    }

    #[test]
    fn clustered_short_track_flag_is_explicit() {
        assert_eq!(parse("git branch -tf feat/a origin/main"), None);
        assert_eq!(parse("git branch -ft feat/a origin/main"), None);
        assert_eq!(parse("git branch -qt feat/a origin/main"), None);
        assert_eq!(
            parse("git branch -f feat/a origin/main"),
            pair("feat/a", "origin/main")
        );
    }

    #[test]
    fn full_ref_spellings_of_a_remote_tracking_start_are_denied() {
        let (_t, work) = setup("track-remote-spell");
        for start in [
            "origin/main",
            "remotes/origin/main",
            "refs/remotes/origin/main",
        ] {
            let f = check(&work, &format!("git switch -c feat/a {start}"));
            assert_eq!(f.len(), 1, "{start}");
        }
    }

    #[test]
    fn track_option_start_omitted_or_other_operations_are_not_targets() {
        for src in [
            "git switch --no-track -c feat/a origin/main",
            "git switch -c feat/a --no-track origin/main",
            "git switch --track -c feat/a origin/main",
            "git checkout --no-track -b feat/a origin/main",
            "git checkout -t -b feat/a origin/main",
            "git branch --no-track feat/a origin/main",
            "git branch --track=direct feat/a origin/main",
            "git branch -t feat/a origin/main",
            "git switch -c feat/a",
            "git checkout -b feat/a",
            "git branch feat/a",
            "git switch origin/main",
            "git checkout origin/main",
            "git checkout origin/main -- a.txt",
            "git branch -d feat/a",
            "git branch -m feat/a origin/main",
            "git branch -u origin/main feat/a",
            "git worktree add -b feat/a ../wt origin/main",
        ] {
            assert_eq!(parse(src), None, "{src}");
        }
    }

    fn setup(tag: &str) -> (TempDir, std::path::PathBuf) {
        let t = TempDir::new(tag);
        let remote = t.0.join("remote.git");
        let work = t.0.join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(&t.0, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git_init(&work);
        git(&work, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(
            &work,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&work, &["push", "-q", "origin", "main"]);
        git(&work, &["fetch", "-q", "origin"]);
        (t, work)
    }

    fn check(work: &std::path::Path, cmd: &str) -> Vec<Finding> {
        let input = Input {
            tool_name: "Bash".into(),
            command: cmd.into(),
            run_in_background: false,
            prompt: String::new(),
            description: String::new(),
            model: String::new(),
            subagent_type: String::new(),
            skill: String::new(),
            args: String::new(),
            file_path: String::new(),
            cwd: work.to_path_buf(),
            session_id: String::new(),
        };
        GuardTrackRemote.check(&input, &Shell::parse(cmd))
    }

    #[test]
    fn remote_tracking_start_is_denied_with_the_fix() {
        let (_t, work) = setup("track-remote-deny");
        let f = check(&work, "git switch -c feat/a origin/main");
        let [Finding::Deny(msg)] = f.as_slice() else {
            panic!("deny が 1 件ではない: {}", f.len())
        };
        assert!(msg.contains("--no-track"), "{msg}");
        assert!(
            msg.contains("git switch --no-track -c feat/a origin/main"),
            "{msg}"
        );
    }

    #[test]
    fn local_start_or_no_track_is_allowed() {
        let (_t, work) = setup("track-remote-allow");
        for cmd in [
            "git switch -c feat/a main",
            "git switch --no-track -c feat/a origin/main",
            "git branch feat/a main",
        ] {
            assert!(check(&work, cmd).is_empty(), "{cmd}");
        }
    }
}
