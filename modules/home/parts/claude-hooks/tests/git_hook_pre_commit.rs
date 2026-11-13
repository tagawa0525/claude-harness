//! `claude-hooks git-hook pre-commit` を、グローバルの core.hooksPath に置いた hook のファイル（中身を呼ぶだけの sh。
//! ADR-0086）から git commit で呼び、次の順で検査することを確かめる。main への直接のコミットを止める（Claude Code の
//! セッションだけ）、マージを締めるコミットでは隣の pre-merge-commit を呼ぶ、ローカルの pre-commit があればそれと
//! 機密情報の検査だけ、.pre-commit-config.yaml があれば prek、fork では機密情報の検査だけ、ほかは既定の検査と
//! ja-terms staged と機密情報の検査。外のコマンド（gitleaks、ja-terms、prek）は偽のコマンドにする。本物での判定は
//! modules/home/parts/tests/ の pre-commit-*.sh、main-commit-gates.sh、ja-terms.sh

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
            "pre-commit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let hooks = root.join("global-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nexec \"{}\" git-hook pre-commit \"$@\"\n",
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
        repo.git(&["init", "-q", "-b", "main", "work"], &repo.root);
        repo
    }

    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    fn log(&self) -> PathBuf {
        self.root.join("tools.log")
    }

    /// 偽のツールを name で PATH に置く
    fn tool(&self, name: &str) {
        place_fake_tool(
            &self
                .root
                .join("bin")
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        );
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        forget_git_env(&mut cmd);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("FAKE_") {
                cmd.env_remove(key);
            }
        }
        let mut path = vec![self.root.join("bin")];
        path.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        cmd.args(args)
            .current_dir(self.work())
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env("CLAUDECODE", "1")
            .env("CLAUDE_HOOKS_GITLEAKS", fake_tool())
            .env("CLAUDE_HOOKS_JA_TERMS", fake_tool())
            .env("FAKE_TOOL_LOG", self.log())
            .env("FAKE_TOOL_OUT", "[]")
            .env_remove("ALLOW_SECRET_COMMIT");
        cmd
    }

    fn git(&self, args: &[&str], dir: &Path) -> Output {
        let mut cmd = self.command(args);
        let out = cmd.current_dir(dir).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    /// path を書いて足し、env を付けてコミットしようとする
    fn commit(&self, path: &str, env: &[(&str, &str)], remove: &[&str]) -> Output {
        std::fs::write(self.work().join(path), format!("{path}\n")).unwrap();
        self.git(&["add", "--", path], &self.work());
        let mut cmd = self.command(&["commit", "-q", "-m", "feat: x"]);
        for (k, v) in env {
            cmd.env(k, v);
        }
        for k in remove {
            cmd.env_remove(k);
        }
        cmd.output().unwrap()
    }

    /// フックを通さずに最初のコミットを作り、feat/x に移る
    fn start_feature(&self) {
        self.git(
            &[
                "commit",
                "-q",
                "--allow-empty",
                "--no-verify",
                "-m",
                "chore: start",
            ],
            &self.work(),
        );
        self.git(&["switch", "-q", "-c", "feat/x"], &self.work());
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

fn said(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const STAGED_SCAN: &str =
    "git --staged --no-banner --redact --log-level error --report-format json --report-path - .";

#[test]
fn commits_on_main_are_stopped_in_claude_code_sessions() {
    let repo = Repo::new();
    repo.git(
        &[
            "commit",
            "-q",
            "--allow-empty",
            "--no-verify",
            "-m",
            "chore: start",
        ],
        &repo.work(),
    );
    let out = repo.commit("a.txt", &[], &[]);
    assert!(!out.status.success());
    assert!(
        said(&out).contains("main / master ブランチへの直接コミットは禁止されています"),
        "{}",
        said(&out)
    );
}

#[test]
fn main_commits_outside_claude_code_and_the_first_commit_pass() {
    let repo = Repo::new();
    // HEAD の無いリポジトリの最初のコミット
    let first = repo.commit("a.txt", &[], &[]);
    assert!(first.status.success(), "{}", said(&first));
    let manual = repo.commit("b.txt", &[], &["CLAUDECODE"]);
    assert!(manual.status.success(), "{}", said(&manual));
}

#[test]
fn a_flake_lock_only_change_on_main_passes() {
    let repo = Repo::new();
    std::fs::write(repo.work().join("flake.lock"), "{}\n").unwrap();
    repo.git(&["add", "flake.lock"], &repo.work());
    repo.git(
        &["commit", "-q", "--no-verify", "-m", "chore: start"],
        &repo.work(),
    );
    let out = repo.commit("flake.lock", &[], &[]);
    assert!(out.status.success(), "{}", said(&out));
}

#[test]
fn a_feature_commit_runs_ja_terms_staged_and_the_secret_scan() {
    let repo = Repo::new();
    repo.start_feature();
    let out = repo.commit("a.txt", &[], &[]);
    assert!(out.status.success(), "{}", said(&out));
    let calls = repo.calls();
    assert!(calls.contains(&"staged".to_string()), "{calls:?}");
    assert_eq!(
        calls.last().map(String::as_str),
        Some(STAGED_SCAN),
        "{calls:?}"
    );
}

#[test]
fn a_failing_term_check_stops_the_commit_after_the_secret_scan() {
    let repo = Repo::new();
    repo.start_feature();
    // 偽の ja-terms と gitleaks は同じ偽のコマンドなので、ja-terms だけを落とすには名前を分ける
    let ja_terms = repo
        .root
        .join(format!("bin/ja-terms{}", std::env::consts::EXE_SUFFIX));
    place_fake_tool(&ja_terms);
    let out = repo.commit(
        "a.txt",
        &[
            ("CLAUDE_HOOKS_JA_TERMS", ja_terms.to_str().unwrap()),
            ("FAKE_JA_TERMS_EXIT", "1"),
        ],
        &[],
    );
    assert!(!out.status.success());
    let calls = repo.calls();
    assert!(calls.contains(&"ja-terms staged".to_string()), "{calls:?}");
    assert_eq!(
        calls.last().map(String::as_str),
        Some(STAGED_SCAN),
        "{calls:?}"
    );
}

#[test]
fn a_project_pre_commit_config_runs_prek() {
    let repo = Repo::new();
    repo.start_feature();
    repo.tool("prek");
    std::fs::write(repo.work().join(".pre-commit-config.yaml"), "repos: []\n").unwrap();
    let out = repo.commit("a.txt", &[("FAKE_PREK_EXIT", "1")], &[]);
    assert!(!out.status.success());
    let calls = repo.calls();
    assert!(
        calls.contains(&"prek run --hook-stage pre-commit".to_string()),
        "{calls:?}"
    );
}

#[test]
fn a_local_pre_commit_replaces_the_default_checks_but_not_the_secret_scan() {
    let repo = Repo::new();
    repo.start_feature();
    let hooks = repo.work().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let local = hooks.join("pre-commit");
    std::fs::write(
        &local,
        format!("#!/bin/sh\necho local >> \"{}\"\n", sh_path(&repo.log())),
    )
    .unwrap();
    executable(&local);
    let out = repo.commit("a.txt", &[], &[]);
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(repo.calls(), ["local", STAGED_SCAN]);
}

#[test]
fn a_failing_local_pre_commit_stops_the_commit() {
    let repo = Repo::new();
    repo.start_feature();
    let hooks = repo.work().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let local = hooks.join("pre-commit");
    std::fs::write(&local, "#!/bin/sh\nexit 3\n").unwrap();
    executable(&local);
    let out = repo.commit("a.txt", &[], &[]);
    assert!(!out.status.success());
    assert!(repo.calls().is_empty(), "{:?}", repo.calls());
}

#[test]
fn forks_only_get_the_secret_scan() {
    let repo = Repo::new();
    repo.start_feature();
    repo.git(
        &["remote", "add", "upstream", "https://example.com/u/r.git"],
        &repo.work(),
    );
    let out = repo.commit("a.txt", &[], &[]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("fork"), "{}", said(&out));
    assert_eq!(repo.calls(), [STAGED_SCAN]);
}

#[test]
fn a_commit_that_concludes_a_merge_runs_the_pre_merge_commit_beside_it() {
    let repo = Repo::new();
    repo.start_feature();
    let marker = repo.root.join("pre-merge-commit.ran");
    let sibling = repo.root.join("global-hooks/pre-merge-commit");
    std::fs::write(
        &sibling,
        format!("#!/bin/sh\necho ran >> \"{}\"\n", sh_path(&marker)),
    )
    .unwrap();
    executable(&sibling);
    // feat/x と main を分けて、競合させる
    std::fs::write(repo.work().join("c.txt"), "feat\n").unwrap();
    repo.git(&["add", "c.txt"], &repo.work());
    repo.git(
        &["commit", "-q", "--no-verify", "-m", "feat: c"],
        &repo.work(),
    );
    repo.git(&["switch", "-q", "main"], &repo.work());
    std::fs::write(repo.work().join("c.txt"), "main\n").unwrap();
    repo.git(&["add", "c.txt"], &repo.work());
    repo.git(
        &["commit", "-q", "--no-verify", "-m", "chore: c"],
        &repo.work(),
    );
    let merge = repo
        .command(&["merge", "-q", "--no-ff", "feat/x"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "競合するはず");
    std::fs::write(repo.work().join("c.txt"), "both\n").unwrap();
    repo.git(&["add", "c.txt"], &repo.work());
    let out = repo
        .command(&["commit", "-q", "--no-edit"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", said(&out));
    assert!(marker.exists(), "{}", said(&out));
}

#[test]
fn a_failing_pre_merge_commit_stops_the_commit_that_concludes_a_merge() {
    let repo = Repo::new();
    repo.start_feature();
    let sibling = repo.root.join("global-hooks/pre-merge-commit");
    std::fs::write(&sibling, "#!/bin/sh\nexit 4\n").unwrap();
    executable(&sibling);
    std::fs::write(repo.work().join("c.txt"), "feat\n").unwrap();
    repo.git(&["add", "c.txt"], &repo.work());
    repo.git(
        &["commit", "-q", "--no-verify", "-m", "feat: c"],
        &repo.work(),
    );
    repo.git(&["switch", "-q", "main"], &repo.work());
    std::fs::write(repo.work().join("c.txt"), "main\n").unwrap();
    repo.git(&["add", "c.txt"], &repo.work());
    repo.git(
        &["commit", "-q", "--no-verify", "-m", "chore: c"],
        &repo.work(),
    );
    let merge = repo
        .command(&["merge", "-q", "--no-ff", "feat/x"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "競合するはず");
    std::fs::write(repo.work().join("c.txt"), "both\n").unwrap();
    repo.git(&["add", "c.txt"], &repo.work());
    let out = repo
        .command(&["commit", "-q", "--no-edit"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    // マージ結果の検査に通らなければ、既定の検査も機密情報の検査も始めない
    assert!(repo.calls().is_empty(), "{:?}", repo.calls());
}
