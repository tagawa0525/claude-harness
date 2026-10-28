//! guard-git-reset-hard: 未コミットの変更がある worktree での `git reset --hard` を止める
//!
//! `reset --hard` は追跡しているファイルの未コミットの変更を戻せない形で消す。worktree は並行するほかのセッションと
//! 共有しうるので（メインの worktree など）、変更がそのセッションのものとは限らない。
//!
//! reset が走りうるディレクトリをすべて検査する。前に `cd` が無いか、絶対パスの `git -C` / `env -C` で指していれば、
//! 決まるディレクトリだけ。そうでなければ、hook の cwd（絶対パスの `env -C` で包んだ `bash -c` の中なら、その先）と、
//! 前の各 `cd` の直前のディレクトリと、すべての `cd` が通ったときのディレクトリ。`cd` が失敗すると、`;` の後の reset は最後に通った `cd` の先で走るため（`cd <一時のリポジトリ>
//! && …; git reset --hard` が、移れなかったときにメインの worktree を戻した）。書き方からディレクトリを決められない形
//! （変数のパス、pushd / popd、`--git-dir` / `--work-tree`、`GIT_DIR` / `GIT_WORK_TREE`、`--hard` を含む文字列を
//! 実行するプログラムを決められないコマンド）は止める側に倒す（ADR-0025 と同じ）。追跡していないファイルは数えない。
//! `git status` が失敗する worktree も止める側に倒す。比べた案と、見ない形は ADR-0062。

use super::{Rule, escape_hint};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};
use crate::unwrap::EnvChange;
use std::path::{Path, PathBuf};

pub struct GuardGitResetHard;

const ESCAPE: &str = "ALLOW_GIT_RESET_HARD";

/// 拒否の文言に挙げる変更の数（worktree ごと）
const SHOWN_CHANGES: usize = 5;

/// `reset --hard`（git が受け付ける `--ha` などの省略形、`-c alias.<名前>=` で書いた alias を含む）か
fn is_hard_reset(cmd: &Cmd) -> bool {
    let is_hard = |t: &str| t.len() >= 3 && "--hard".starts_with(t);
    match cmd.git_subcommand() {
        Some(("reset", args)) => args.iter().any(|a| is_hard(&a.text)),
        Some((sub, _)) => cmd.git_config_overrides().iter().any(|c| {
            c.strip_prefix(&format!("alias.{sub}=")).is_some_and(|v| {
                v.split_whitespace().any(|w| w == "reset") && v.split_whitespace().any(is_hard)
            })
        }),
        None => false,
    }
}

/// 字面のままではディレクトリが決まらない引数（変数やコマンドの置き換え）
fn unexpanded(text: &str) -> bool {
    text.contains('$') || text.contains('`')
}

fn absolute(text: &str) -> bool {
    text.starts_with('/') || text.starts_with('~')
}

impl Rule for GuardGitResetHard {
    fn name(&self) -> &'static str {
        "guard-git-reset-hard"
    }

    fn escape(&self) -> Option<&'static str> {
        Some(ESCAPE)
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut undetermined: Vec<String> = Vec::new();
        let mut dirty: Vec<(PathBuf, String)> = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        // 実行するプログラムを決められない形（`$x`、eval）は、文字列に --hard があれば止める側に倒す。reset は
        // preset などの語の中にも現れるので見ない
        if shell.runs_program("--hard", |_| false) {
            undetermined
                .push("実行するプログラムを決められないコマンド（変数や eval）".to_string());
        }
        // export や単独の代入で置いた GIT_DIR / GIT_WORK_TREE は、後のすべての git の対象を変える
        let git_dir_assigned = shell
            .assignments()
            .iter()
            .any(|(n, _)| n == "GIT_DIR" || n == "GIT_WORK_TREE");
        for cmd in shell.commands() {
            if !is_hard_reset(cmd) || shell.runs_under_ssh_or_login(cmd) {
                continue;
            }
            let before = |c: &&Cmd| c.start < cmd.start && cmd.scope.starts_with(&c.scope);
            let preceding_cds: Vec<&Cmd> = shell
                .commands()
                .iter()
                .filter(before)
                .filter(|c| c.changes_dir())
                .collect();
            // 包んだ `env -C x bash -c '…'` の x も、cmd のディレクトリを決める
            let outer_dirs = shell.commands().iter().filter(before).filter(|c| {
                c.spawns
                    .is_some_and(|id| cmd.scope.get(c.scope.len()) == Some(&id))
            });
            let git_dirs: &[String] = cmd.git.as_ref().map_or(&[], |g| g.dirs.as_slice());
            let own_pins: Vec<&String> = cmd.dirs.iter().chain(git_dirs).collect();
            let pinned = own_pins.iter().any(|d| absolute(d));
            let mut why = Vec::new();
            if own_pins.iter().any(|d| unexpanded(d))
                || outer_dirs
                    .clone()
                    .any(|c| c.dirs.iter().any(|d| unexpanded(d)))
                || (!pinned
                    && preceding_cds
                        .iter()
                        .any(|c| c.args.iter().any(|a| unexpanded(&a.text))))
            {
                why.push("変数のディレクトリ");
            }
            if !pinned
                && shell
                    .commands()
                    .iter()
                    .filter(before)
                    .any(|c| c.name == "pushd" || c.name == "popd")
            {
                why.push("pushd / popd");
            }
            let sub = cmd
                .git
                .as_ref()
                .and_then(|g| g.sub)
                .unwrap_or(cmd.args.len());
            if git_dir_assigned
                || cmd.args[..sub]
                    .iter()
                    .any(|a| a.text.starts_with("--git-dir") || a.text.starts_with("--work-tree"))
                || cmd.env.iter().any(
                    |e| matches!(e, EnvChange::Set(n, _) if n == "GIT_DIR" || n == "GIT_WORK_TREE"),
                )
            {
                why.push("--git-dir / --work-tree / GIT_DIR / GIT_WORK_TREE");
            }
            if !why.is_empty() {
                undetermined.push(why.join("、"));
                continue;
            }
            // 前に cd が無ければ、走るディレクトリは包んだ -C と git -C だけで決まる。包んだ絶対パスの `env -C x bash -c
            // '…'` の中の cd が失敗すると、reset は hook の cwd でなく x（か、中で前に通った cd の先）で走る
            let outer_pin = outer_dirs
                .filter(|c| c.dirs.iter().any(|d| absolute(d)))
                .map(|c| c.start)
                .max();
            let fallback_cds: Vec<&&Cmd> = preceding_cds
                .iter()
                .filter(|c| outer_pin.is_none_or(|start| c.start > start))
                .collect();
            let mut dirs = vec![shell.target_dir(cmd, &input.cwd)];
            if !pinned && !fallback_cds.is_empty() {
                if outer_pin.is_none() {
                    dirs.push(input.cwd.clone());
                }
                dirs.extend(fallback_cds.iter().map(|c| shell.target_dir(c, &input.cwd)));
            }
            for dir in dirs {
                if seen.contains(&dir) || git::not_a_directory(&dir) {
                    continue;
                }
                seen.push(dir.clone());
                match status(&dir) {
                    Status::Clean | Status::NotARepo => {}
                    Status::Dirty(s) => dirty.push((dir, s)),
                    Status::Failed => {
                        undetermined.push(format!("{} で git status が失敗する", dir.display()))
                    }
                }
            }
        }
        if dirty.is_empty() && undetermined.is_empty() {
            return Vec::new();
        }
        let mut msg = String::new();
        if !dirty.is_empty() {
            let listing: String = dirty
                .iter()
                .map(|(dir, status)| {
                    let lines: Vec<&str> = status.lines().collect();
                    let files: String = lines
                        .iter()
                        .take(SHOWN_CHANGES)
                        .map(|l| format!("\n     {l}"))
                        .collect();
                    let more = match lines.len().saturating_sub(SHOWN_CHANGES) {
                        0 => String::new(),
                        n => format!("\n     …ほか {n} 件"),
                    };
                    format!("\n  {}:{files}{more}", dir.display())
                })
                .collect();
            msg.push_str(&format!(
                "git reset --hard は、次の worktree の未コミットの変更を戻せない形で消します（ほかのセッションの変更かもしれません）:{listing}\n\
                 前の cd が失敗すると、reset は最後に通った cd の先（無ければ hook の cwd）で走るので、それらも検査しています。\n"
            ));
        }
        if !undetermined.is_empty() {
            msg.push_str(&format!(
                "git reset --hard が走るディレクトリを書き方から決められません（{}）。\n",
                undetermined.join("、")
            ));
        }
        msg.push_str(&format!(
            "対象は git -C <絶対パス> reset --hard で指してください（cd の成否と変数に依りません）。残す変更は WIP としてコミットしてください。\n{}",
            escape_hint(ESCAPE)
        ));
        vec![Finding::Deny(msg)]
    }
}

enum Status {
    Clean,
    Dirty(String),
    NotARepo,
    Failed,
}

fn status(dir: &Path) -> Status {
    // サブモジュールの中の変更も挙げる（submodule.recurse が設定にあれば reset --hard が消す）。--no-optional-locks で、
    // ほかのセッションが使う worktree の index.lock を取らない
    let args = [
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=no",
    ];
    match git::git(dir, &args) {
        Some(s) if s.is_empty() => Status::Clean,
        Some(s) => Status::Dirty(s),
        // worktree の外（git が無いディレクトリ）では reset も失敗するので見ない。worktree の中で status だけが
        // 失敗するなら、reset が何を消すかを確かめられない
        None if git::git(dir, &["rev-parse", "--is-inside-work-tree"]).as_deref()
            == Some("true") =>
        {
            Status::Failed
        }
        None => Status::NotARepo,
    }
}
