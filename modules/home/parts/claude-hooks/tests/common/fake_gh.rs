//! `claude-hooks script <名前>` の結合テストの部品。偽の gh（examples/fake_tool.rs）を PATH の先頭に置いてスクリプトを
//! 起動し、gh に渡した引数と環境（GH_REPO、GH_HOST）を記録する

use super::{executable, fake_tool};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct FakeGh {
    root: PathBuf,
}

impl Drop for FakeGh {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl FakeGh {
    pub fn new() -> FakeGh {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "fake-gh-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let gh = root
            .join("bin")
            .join(format!("gh{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(fake_tool(), &gh).unwrap();
        executable(&gh);
        FakeGh { root }
    }

    /// `claude-hooks script <script> <args>...` を起動する。偽の gh は gh_out を標準出力に書き、gh_exit で終わる
    pub fn run(&self, script: &str, args: &[&str], gh_out: &str, gh_exit: i32) -> Output {
        let path = std::env::join_paths(
            std::iter::once(self.root.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["script", script])
            .args(args)
            .current_dir(&self.root)
            .env("PATH", path)
            // 呼んだ側の環境の対象（GH_REPO、GH_HOST）に向かず、URL のリポジトリに向くことを確かめる
            .env("GH_REPO", "other/repo")
            .env("GH_HOST", "ghe.example.com")
            .env("FAKE_GH_LOG", self.root.join("gh.log"))
            .env("FAKE_GH_ENV", self.root.join("gh-env.log"))
            .env("FAKE_GH_ENV_NAMES", "GH_REPO,GH_HOST")
            .env("FAKE_GH_OUT", gh_out)
            .env("FAKE_GH_EXIT", gh_exit.to_string())
            .output()
            .unwrap()
    }

    /// gh に渡した引数（1 回の起動で 1 行）
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.root.join("gh.log")).unwrap_or_default()
    }

    /// gh に渡した GH_REPO と GH_HOST（1 回の起動で 2 行）
    pub fn env(&self) -> String {
        std::fs::read_to_string(self.root.join("gh-env.log")).unwrap_or_default()
    }
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}
