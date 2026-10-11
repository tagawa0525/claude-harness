//! 結合テストで共有する、一時的な git リポジトリと CLI の呼び出し。

// テストのファイルごとに使う道具が違うので、使わないものがあっても警告しない
#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// テストで作る一時的なリポジトリ。落ちたときも含めて、片付けで消す。
pub struct TempRepo {
    pub dir: PathBuf,
}

impl TempRepo {
    /// プロセスと時刻とテストの名前で一意にした、main ブランチのリポジトリ。README.md の初期コミットを持つ。
    /// 書き換えのたびに git が post-rewrite に渡す「旧 新」の対応を `.git/rewritten` に残す hook を置く
    /// （実物の rebase と amend が出す入力の形をそのまま使うため）
    pub fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("commit-refs-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let repo = TempRepo { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        let hooks = repo.dir.join(".git/capture-hooks");
        fs::create_dir(&hooks).unwrap();
        let hook = hooks.join("post-rewrite");
        fs::write(
            &hook,
            "#!/bin/sh\ncat > \"$(git rev-parse --git-dir)/rewritten\"\n",
        )
        .unwrap();
        make_executable(&hook);
        repo.write("README.md", "init\n");
        repo.commit_all("chore: init");
        repo
    }

    /// git を走らせ、成功を確かめて標準出力を返す。親の git（hook）から受け継いだ GIT_ で始まる環境変数は外す
    /// （外さないと、hook の中で走るテストの操作が、hook を呼んだ本物のリポジトリに効く）
    pub fn git(&self, args: &[&str]) -> String {
        self.git_env(&[], args)
    }

    /// 環境変数 `env` を足して git を走らせる（日時を決めてコミットするときなど）
    pub fn git_env(&self, env: &[(&str, &str)], args: &[&str]) -> String {
        let hooks = self.dir.join(".git/capture-hooks");
        let mut command = Command::new("git");
        command
            .current_dir(&self.dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .arg("-c")
            .arg(format!("core.hooksPath={}", hooks.display()))
            .args(args);
        isolate(&mut command);
        command.envs(env.iter().copied());
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    pub fn path(&self, path: &str) -> PathBuf {
        self.dir.join(path)
    }

    pub fn write(&self, path: &str, text: &str) {
        let file = self.path(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.path(path)).unwrap()
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// ファイルに 1 行足してコミットする
    pub fn record(&self, path: &str, line: &str) {
        let mut text = fs::read_to_string(self.path(path)).unwrap_or_default();
        text.push_str(line);
        text.push('\n');
        self.write(path, &text);
        self.commit_all(&format!("docs: {line}"));
    }

    /// ファイルを 1 つ作ってコミットし、そのコミットの番号を返す
    pub fn commit_file(&self, path: &str, message: &str) -> String {
        self.write(path, &format!("{message}\n"));
        self.commit_all(message);
        self.head()
    }

    pub fn head(&self) -> String {
        self.rev("HEAD")
    }

    pub fn rev(&self, rev: &str) -> String {
        self.git(&["rev-parse", rev]).trim().to_string()
    }

    /// git が post-rewrite に渡した対応
    pub fn rewritten(&self) -> String {
        fs::read_to_string(self.path(".git/rewritten")).unwrap()
    }

    /// 対応表を一時的なリポジトリの外（.git の下）に書き、そのパスを返す
    pub fn map_file(&self, text: &str) -> String {
        let path = self.path(".git/test-map");
        fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_string()
    }

    pub fn remap_in(&self, sub: &str, args: &[&str], stdin: &str) -> Output {
        let mut all = vec!["remap"];
        all.extend_from_slice(args);
        run(&self.dir.join(sub), &all, stdin)
    }

    pub fn remap(&self, args: &[&str]) -> Output {
        self.remap_in("", args, "")
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        // 片付けに失敗しても、テストの結果は変えない（一時的なディレクトリが残るだけ）
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn isolate(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
}

pub fn run(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_commit-refs"));
    command
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate(&mut command);
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
}

pub fn assert_error(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
    assert!(stderr(output).contains("ERROR:"), "{}", stderr(output));
}

pub const NEW4: &str = "4444444444444444444444444444444444444444";
pub const NEW5: &str = "5555555555555555555555555555555555555555";
pub const OLD_A: &str = "abcdef0123456789abcdef0123456789abcdef01";
pub const OLD_B: &str = "abcdef0fedcba9876543210fedcba9876543210f";
pub const ZERO: &str = "0000000000000000000000000000000000000000";
