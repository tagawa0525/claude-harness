//! `claude-hooks script <名前>` の結合テストの部品。偽の gh（examples/fake_tool.rs）を PATH の先頭に置いてスクリプトを
//! 起動し、gh に渡した引数と環境（GH_REPO、GH_HOST）を記録する

use super::{fake_tool, place_fake_tool};
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
        place_fake_tool(&gh);
        FakeGh { root }
    }

    /// `claude-hooks script <script> <args>...` を起動する。偽の gh は gh_out を標準出力に書き、gh_exit で終わる
    pub fn run(&self, script: &str, args: &[&str], gh_out: &str, gh_exit: i32) -> Output {
        self.run_calls(script, args, &[(gh_out, gh_exit)])
    }

    /// [`FakeGh::run`] と同じだが、gh の n 回目の起動は calls の n 番目（0 から）の標準出力と終了コードを返す。calls より
    /// 多く起動されたら最後のものを返す
    pub fn run_calls(&self, script: &str, args: &[&str], calls: &[(&str, i32)]) -> Output {
        self.run_calls_with_env(script, args, calls, &[])
    }

    /// [`FakeGh::run_calls`] と同じだが、スクリプトに env の環境変数を足して起動する
    pub fn run_calls_with_env(
        &self,
        script: &str,
        args: &[&str],
        calls: &[(&str, i32)],
        env: &[(&str, &str)],
    ) -> Output {
        let path = std::env::join_paths(
            std::iter::once(self.root.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        cmd.args(["script", script])
            .args(args)
            .current_dir(&self.root)
            .env("PATH", path)
            // 呼んだ側の環境の対象（GH_REPO、GH_HOST）に向かず、URL のリポジトリに向くことを確かめる
            .env("GH_REPO", "other/repo")
            .env("GH_HOST", "ghe.example.com")
            .env("FAKE_GH_LOG", self.root.join("gh.log"))
            .env("FAKE_GH_ENV", self.root.join("gh-env.log"))
            .env("FAKE_GH_ENV_NAMES", "GH_REPO,GH_HOST")
            .env("FAKE_GH_CALLS", self.root.join("gh-calls"));
        let (last_out, last_exit) = calls.last().expect("calls は 1 つ以上");
        cmd.env("FAKE_GH_OUT", last_out)
            .env("FAKE_GH_EXIT", last_exit.to_string());
        for (i, (out, exit)) in calls.iter().enumerate() {
            cmd.env(format!("FAKE_GH_OUT_{}", i + 1), out)
                .env(format!("FAKE_GH_EXIT_{}", i + 1), exit.to_string());
        }
        cmd.envs(env.iter().copied());
        cmd.output().unwrap()
    }

    /// gh に渡した引数（1 回の起動で、空白でつないで改行で終える。引数が改行を含めば複数の行になる）
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
