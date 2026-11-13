//! `claude-hooks git-hook commit-msg` を、グローバルの core.hooksPath に置いた hook のファイル（中身を呼ぶだけの sh。
//! ADR-0086）から git commit で呼び、Claude Code のセッションのコミットのメッセージを ja-terms commit-msg に渡すこと、
//! ローカルの commit-msg、Claude Code の外、fork では渡さないことを確かめる。ja-terms は偽のコマンド
//! （examples/fake_tool.rs）にする。本物の ja-terms での判定は modules/home/parts/tests/commit-msg-conventions.sh

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

/// examples/fake_tool の実行ファイル（テストの実行ファイルは target/<profile>/deps にある）。`cargo test` は examples も
/// ビルドする（`--test` で絞ると作らないので、先に `cargo build --examples` を流す）
fn fake_tool() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap();
    profile
        .join("examples")
        .join(format!("fake_tool{}", std::env::consts::EXE_SUFFIX))
}

/// sh に書くパス（Windows の `\` は sh の中で消えるので `/` にする）
fn sh_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// 実行の権限を付ける（Windows には無い）
fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
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
            "commit-msg-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let hooks = root.join("global-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("commit-msg");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nexec \"{}\" git-hook commit-msg \"$@\"\n",
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
        repo
    }

    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    fn log(&self) -> PathBuf {
        self.root.join("ja-terms.log")
    }

    fn command(&self, args: &[&str], dir: &Path) -> Command {
        let mut cmd = Command::new("git");
        // 呼んだ側のリポジトリ（run-checks.sh を実行する git の hook の中の GIT_DIR、GIT_INDEX_FILE など）を見せない。
        // 残すと、テストの init や commit が外のリポジトリに当たる
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        cmd.args(args)
            .current_dir(dir)
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env("CLAUDECODE", "1")
            .env("CLAUDE_HOOKS_JA_TERMS", fake_tool())
            .env("FAKE_TOOL_LOG", self.log())
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

    /// 空のコミットを、env を足して作ろうとする
    fn commit(&self, env: &[(&str, &str)], remove: &[&str]) -> Output {
        let mut cmd = self.command(
            &["commit", "-q", "--allow-empty", "-m", "feat: 足す"],
            &self.work(),
        );
        for (k, v) in env {
            cmd.env(k, v);
        }
        for k in remove {
            cmd.env_remove(k);
        }
        cmd.output().unwrap()
    }

    /// ローカルの commit-msg のパス（init の template に hooks が無いこともあるので、ディレクトリを作る）
    fn local_hook(&self) -> PathBuf {
        let hooks = self.work().join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        hooks.join("commit-msg")
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn commits(&self) -> usize {
        let out = self.git(&["rev-list", "--count", "--all"], &self.work());
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }
}

#[test]
fn the_message_file_goes_to_ja_terms_commit_msg() {
    let repo = Repo::new();
    let out = repo.commit(&[], &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = repo.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].starts_with("commit-msg "), "{calls:?}");
    assert!(calls[0].ends_with("COMMIT_EDITMSG"), "{calls:?}");
}

#[test]
fn a_failing_check_stops_the_commit() {
    let repo = Repo::new();
    let out = repo.commit(&[("FAKE_TOOL_EXIT", "1")], &[]);
    assert!(!out.status.success());
    // 止めたのが検査であること（偽の ja-terms を起動できずに止まったのでないこと）
    assert_eq!(
        repo.calls().len(),
        1,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(repo.commits(), 0);
}

#[test]
fn commits_outside_claude_code_are_not_checked() {
    let repo = Repo::new();
    let out = repo.commit(&[("FAKE_TOOL_EXIT", "1")], &["CLAUDECODE"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(repo.calls().is_empty());
}

#[test]
fn forks_are_not_checked() {
    let repo = Repo::new();
    repo.git(
        &["remote", "add", "upstream", "https://example.com/u/r.git"],
        &repo.work(),
    );
    let out = repo.commit(&[("FAKE_TOOL_EXIT", "1")], &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(repo.calls().is_empty());
    // git は hook の標準出力を標準エラーにつなぐ
    assert!(String::from_utf8_lossy(&out.stderr).contains("fork"));
}

#[test]
fn a_local_commit_msg_hook_runs_instead() {
    let repo = Repo::new();
    let local = repo.local_hook();
    let marker = repo.root.join("local.log");
    std::fs::write(
        &local,
        format!("#!/bin/sh\necho \"$1\" >> \"{}\"\n", sh_path(&marker)),
    )
    .unwrap();
    executable(&local);
    let out = repo.commit(&[("FAKE_TOOL_EXIT", "1")], &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(repo.calls().is_empty());
    let got = std::fs::read_to_string(&marker).unwrap();
    assert!(got.trim_end().ends_with("COMMIT_EDITMSG"), "{got}");
}

#[test]
fn a_failing_local_hook_stops_the_commit() {
    let repo = Repo::new();
    let local = repo.local_hook();
    let marker = repo.root.join("local.log");
    std::fs::write(
        &local,
        format!("#!/bin/sh\necho ran >> \"{}\"\nexit 3\n", sh_path(&marker)),
    )
    .unwrap();
    executable(&local);
    let out = repo.commit(&[], &[]);
    assert!(!out.status.success());
    assert!(marker.exists(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(repo.calls().is_empty());
    assert_eq!(repo.commits(), 0);
}

// Windows は環境変数が無ければ PATH の ja-terms を使う（ADR-0085）
#[cfg(not(windows))]
#[test]
fn without_ja_terms_the_commit_stops() {
    let repo = Repo::new();
    let out = repo.commit(&[], &["CLAUDE_HOOKS_JA_TERMS"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("ja-terms"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(repo.commits(), 0);
}

#[test]
fn every_argument_reaches_the_local_hook() {
    // 前の sh は `exec "$LOCAL_HOOK" "$@"` で、引数の数を問わず渡した
    let repo = Repo::new();
    let local = repo.local_hook();
    let marker = repo.root.join("local.log");
    std::fs::write(
        &local,
        format!("#!/bin/sh\necho \"$#:$*\" >> \"{}\"\n", sh_path(&marker)),
    )
    .unwrap();
    executable(&local);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .args(["git-hook", "commit-msg", "a", "b"])
        .current_dir(repo.work())
        .env("GIT_CONFIG_GLOBAL", repo.root.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap().trim_end(),
        "2:a b"
    );
}

// Git for Windows は hooks/<名前>.exe も hook として探す
#[cfg(windows)]
#[test]
fn a_local_exe_hook_runs_on_windows() {
    let repo = Repo::new();
    let local = repo.local_hook().with_extension("exe");
    std::fs::copy(fake_tool(), &local).unwrap();
    let out = repo.commit(&[], &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // ローカルの hook（偽のコマンドのコピー）だけが呼ばれ、ja-terms に渡す `commit-msg` の引数は付かない
    let calls = repo.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].ends_with("COMMIT_EDITMSG"), "{calls:?}");
    assert!(!calls[0].starts_with("commit-msg "), "{calls:?}");
}

#[test]
fn a_local_hook_without_a_shebang_runs_as_sh() {
    // git と POSIX の sh は、shebang の行の無い実行ファイルを sh のスクリプトとして実行する
    let repo = Repo::new();
    let local = repo.local_hook();
    let marker = repo.root.join("local.log");
    std::fs::write(&local, format!("echo ran >> \"{}\"\n", sh_path(&marker))).unwrap();
    executable(&local);
    let out = repo.commit(&[], &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(marker.exists());
}
