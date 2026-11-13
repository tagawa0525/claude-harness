//! `claude-hooks git-hook post-rewrite` を、グローバルの core.hooksPath に置いた hook のファイル（中身を呼ぶだけの sh。
//! ADR-0086）から amend と rebase で呼び、`git config remap.commitRefs true` のリポジトリでだけ、旧と新の対応を
//! commit-refs remap に渡すことを確かめる。commit-refs は偽のコマンド（examples/fake_tool.rs）にする。本物の
//! commit-refs での付け直しは modules/home/parts/tests/post-rewrite-remap.sh

mod common;

use common::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Repo {
    root: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Repo {
    fn new() -> Repo {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "post-rewrite-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let hooks = root.join("global-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("post-rewrite");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nexec \"{}\" git-hook post-rewrite \"$@\"\n",
                sh_path(Path::new(env!("CARGO_BIN_EXE_claude-hooks")))
            ),
        )
        .unwrap();
        executable(&hook);
        std::fs::write(
            root.join("gitconfig"),
            format!("[core]\n\thooksPath = {}\n", sh_path(&hooks)),
        )
        .unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "feat/x", "work"], &repo.root);
        std::fs::write(repo.work().join("a"), "a\n").unwrap();
        repo.git(&["add", "a"], &repo.work());
        repo.git(&["commit", "-q", "-m", "feat: a"], &repo.work());
        repo
    }

    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    fn log(&self) -> PathBuf {
        self.root.join("commit-refs.log")
    }

    fn stdin(&self) -> PathBuf {
        self.root.join("commit-refs.stdin")
    }

    fn opt_in(&self) {
        self.git(&["config", "remap.commitRefs", "true"], &self.work());
    }

    fn command(&self, args: &[&str], dir: &Path) -> Command {
        let mut cmd = Command::new("git");
        forget_git_env(&mut cmd);
        cmd.args(args)
            .current_dir(dir)
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env("CLAUDE_HOOKS_COMMIT_REFS", fake_tool())
            .env("FAKE_TOOL_LOG", self.log())
            .env("FAKE_TOOL_STDIN", self.stdin())
            .env("FAKE_TOOL_OUT", "FILES: 1\nREFS: 2\n")
            .env_remove("FAKE_TOOL_EXIT");
        cmd
    }

    fn git(&self, args: &[&str], dir: &Path) -> Output {
        let out = self.command(args, dir).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn head(&self) -> String {
        let out = self.git(&["rev-parse", "HEAD"], &self.work());
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// HEAD を amend する（git は post-rewrite の失敗で amend を止めないので、成功を確かめる）
    fn amend(&self, env: &[(&str, &str)], remove: &[&str]) -> Output {
        let mut cmd = self.command(&["commit", "-q", "--amend", "-m", "feat: b"], &self.work());
        for (k, v) in env {
            cmd.env(k, v);
        }
        for k in remove {
            cmd.env_remove(k);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// ローカルの post-rewrite のパス（init の template に hooks が無いこともあるので、ディレクトリを作る）
    fn local_hook(&self) -> PathBuf {
        let hooks = self.work().join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        hooks.join("post-rewrite")
    }

    fn saved(&self) -> PathBuf {
        self.work().join(".git/commit-refs-remap.input")
    }
}

/// hook の出力（git は hook の標準出力を標準エラーにつなぐ）
fn hook_output(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn repositories_that_have_not_opted_in_are_left_alone() {
    let repo = Repo::new();
    repo.amend(&[], &[]);
    assert!(repo.calls().is_empty());
}

#[test]
fn an_amend_passes_the_old_and_new_commits_to_commit_refs_remap() {
    let repo = Repo::new();
    repo.opt_in();
    let old = repo.head();
    let out = repo.amend(&[], &[]);
    let new = repo.head();
    assert_eq!(repo.calls(), ["remap"]);
    let input = std::fs::read_to_string(repo.stdin()).unwrap();
    assert!(input.starts_with(&format!("{old} {new}")), "{input}");
    let said = hook_output(&out);
    assert!(said.contains("REFS: 2"), "{said}");
    // 置き換えはコミットしないので、確かめてコミットするよう伝える
    assert!(said.contains("git diff"), "{said}");
}

#[test]
fn nothing_to_commit_is_said_when_no_reference_was_replaced() {
    let repo = Repo::new();
    repo.opt_in();
    let out = repo.amend(&[("FAKE_TOOL_OUT", "FILES: 0\nREFS: 0\n")], &[]);
    let said = hook_output(&out);
    assert!(said.contains("REFS: 0"), "{said}");
    assert!(!said.contains("git diff"), "{said}");
}

#[test]
fn a_failing_remap_keeps_the_input_for_a_manual_retry() {
    let repo = Repo::new();
    repo.opt_in();
    let old = repo.head();
    let out = repo.amend(&[("FAKE_TOOL_EXIT", "1")], &[]);
    let said = hook_output(&out);
    assert!(said.contains("commit-refs remap"), "{said}");
    let saved = std::fs::read_to_string(repo.saved()).unwrap();
    assert!(saved.starts_with(&old), "{saved}");
}

#[test]
fn an_amend_inside_a_rebase_waits_for_the_end_of_the_rebase() {
    let repo = Repo::new();
    repo.opt_in();
    std::fs::write(repo.work().join("b"), "b\n").unwrap();
    repo.git(&["add", "b"], &repo.work());
    repo.git(&["commit", "-q", "-m", "feat: b"], &repo.work());
    repo.git(
        &[
            "rebase",
            "-q",
            "--force-rebase",
            "--exec",
            "git commit -q --amend -m 'feat: c'",
            "HEAD~1",
        ],
        &repo.work(),
    );
    // rebase の途中の amend では呼ばず、rebase の終わりに 1 度だけ呼ぶ
    assert_eq!(repo.calls(), ["remap"]);
}

#[test]
fn a_local_post_rewrite_gets_the_same_input_first() {
    let repo = Repo::new();
    repo.opt_in();
    let local = repo.local_hook();
    let marker = repo.root.join("local.stdin");
    // 呼ばれた順を見るため、偽の commit-refs と同じ記録に書く
    std::fs::write(
        &local,
        format!(
            "#!/bin/sh\ncat >> \"{}\"\necho local >> \"{}\"\nexit 3\n",
            sh_path(&marker),
            sh_path(&repo.log())
        ),
    )
    .unwrap();
    executable(&local);
    repo.amend(&[], &[]);
    // ローカルの hook が失敗しても、付け直しは行う
    assert_eq!(repo.calls(), ["local", "remap"]);
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap(),
        std::fs::read_to_string(repo.stdin()).unwrap()
    );
}

#[test]
fn the_exit_code_of_the_local_hook_is_returned() {
    let repo = Repo::new();
    repo.opt_in();
    let local = repo.local_hook();
    std::fs::write(&local, "#!/bin/sh\ncat > /dev/null\nexit 3\n").unwrap();
    executable(&local);
    // git は post-rewrite の終了コードを見ないので、hook を直に呼ぶ
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
    forget_git_env(&mut cmd);
    let mut child = cmd
        .args(["git-hook", "post-rewrite", "rebase"])
        .current_dir(repo.work())
        .env("GIT_CEILING_DIRECTORIES", &repo.root)
        .env("GIT_CONFIG_GLOBAL", repo.root.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("CLAUDE_HOOKS_COMMIT_REFS", fake_tool())
        .env("FAKE_TOOL_OUT", "REFS: 0\n")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let head = repo.head();
        writeln!(child.stdin.take().unwrap(), "{head} {head}").unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_wrong_argument_is_a_usage_error() {
    let repo = Repo::new();
    let out = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["git-hook", "post-rewrite", "squash"])
        .current_dir(repo.work())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

// Windows は環境変数が無ければ PATH の commit-refs を使う（ADR-0086）
#[cfg(not(windows))]
#[test]
fn without_commit_refs_the_input_is_kept() {
    let repo = Repo::new();
    repo.opt_in();
    let out = repo.amend(&[], &["CLAUDE_HOOKS_COMMIT_REFS"]);
    let said = hook_output(&out);
    assert!(said.contains("commit-refs"), "{said}");
    // commit-refs を起動していないので、書き換えたかを ERROR の行で探させない
    assert!(!said.contains("ERROR"), "{said}");
    assert!(repo.saved().is_file());
}
