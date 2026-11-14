//! `claude-hooks script` のスクリプトが、読む側が先に閉じた標準出力に書くと、panic せず、sh と同じく SIGPIPE で
//! 黙って終わることを確かめる（`git-info.sh | head -2` のように、出力の先頭だけを読む使い方。シェルは 141 と見る）

mod common;

use common::forget_git_env;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[cfg(unix)]
fn a_script_writing_to_a_closed_pipe_ends_by_sigpipe_like_sh() {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("broken-pipe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let ok = forget_git_env(&mut Command::new("git"))
            .args(args)
            .current_dir(&root)
            .env("GIT_CONFIG_GLOBAL", root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ]);

    // 読む側を子の起動の前に閉じ、最初の書き込みから必ず EPIPE になるようにする（起動の後に閉じると、パイプの
    // バッファに収まった出力は失敗しない）
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
        .args(["script", "git-info"])
        .current_dir(&root)
        .env("GIT_CONFIG_GLOBAL", root.join("no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&root);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(out.status.signal(), Some(13), "{:?}: {stderr}", out.status);
}
