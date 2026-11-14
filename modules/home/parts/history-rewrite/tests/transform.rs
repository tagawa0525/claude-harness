//! `history-rewrite transform [語の指定] FILE...` が、作業ツリーのファイルの語を、run の第 1 段と同じ変換で直すこと
//! （ADR-0045、ADR-0088）。既定の辞書は ja-terms の辞書（ビルドに埋めたもの）。
//!
//! 辞書の語（崩れた語）をわざと含むので、.textlintignore で ja-terms の検査から外す。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Work {
    root: PathBuf,
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn forget_git_env(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
}

impl Work {
    fn new() -> Work {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "transform-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("repo")).unwrap();
        let work = Work { root };
        let out = forget_git_env(&mut Command::new("git"))
            .args(["init", "-q"])
            .current_dir(work.repo())
            .env("GIT_CONFIG_GLOBAL", work.root.join("no-global-config"))
            .output()
            .unwrap();
        assert!(out.status.success());
        work
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn write(&self, name: &str, content: &[u8]) {
        let path = self.repo().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.repo().join(name)).unwrap()
    }

    fn transform(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_history-rewrite")))
            .arg("transform")
            .args(args)
            .current_dir(self.repo())
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn ok(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    text(&out.stdout)
}

#[test]
fn files_of_the_working_tree_are_fixed_in_place() {
    let work = Work::new();
    work.write("doc.md", "確かめの記録\n".as_bytes());
    work.write("keep.md", "結果を確かめる\n".as_bytes());
    let out = ok(&work.transform(&["doc.md", "keep.md"]));
    assert_eq!(out, "REPLACED: doc.md\nFILES: 1\n");
    assert_eq!(work.read("doc.md"), "確認の記録\n");
    assert_eq!(work.read("keep.md"), "結果を確かめる\n");
}

#[test]
fn code_and_text_files_are_fixed_across_the_whole_file() {
    let work = Work::new();
    work.write(
        "tool.py",
        "# 確かめの順に掃引する\nprint('ok')\n".as_bytes(),
    );
    ok(&work.transform(&["tool.py"]));
    assert_eq!(
        work.read("tool.py"),
        "# 確認の順にスイープする\nprint('ok')\n"
    );
}

#[test]
fn files_that_are_not_read_are_named() {
    let work = Work::new();
    work.write("latin1.txt", b"caf\xe9 \n");
    work.write("data.bin", b"\0\x01");
    work.write("dict/terms.yml", "pattern: 確かめの\n".as_bytes());
    work.write("plain.md", "確認の記録\n".as_bytes());
    let out = ok(&work.transform(&[
        "--exclude",
        "**/terms.yml",
        "latin1.txt",
        "data.bin",
        "dict/terms.yml",
        "plain.md",
    ]));
    assert_eq!(
        out,
        "SKIPPED: latin1.txt (not UTF-8)\nSKIPPED: data.bin (binary)\nSKIPPED: dict/terms.yml (excluded)\nFILES: 0\n"
    );
}

#[test]
fn only_tables_with_a_fixed_row_are_realigned() {
    let work = Work::new();
    let readme = "# 手順\n\n確かめの結果を書く。結果を確かめる。\n\n| 項目 | 内容 |\n| ---- | ---- |\n| 幅 | 掃引の幅 |\n\n| 崩れた | 表 |\n|-|-|\n| x | y |\n";
    work.write("README.md", readme.as_bytes());
    ok(&work.transform(&["README.md"]));
    assert_eq!(
        work.read("README.md"),
        "# 手順\n\n確認の結果を書く。結果を確かめる。\n\n| 項目 | 内容         |\n| ---- | ------------ |\n| 幅   | スイープの幅 |\n\n| 崩れた | 表 |\n|-|-|\n| x | y |\n"
    );
}

#[test]
fn tables_in_code_fences_are_not_realigned() {
    let work = Work::new();
    work.write(
        "fence.md",
        "```text\n| a | b |\n| - | - |\n| 掃引 | x |\n```\n".as_bytes(),
    );
    ok(&work.transform(&["fence.md"]));
    assert_eq!(
        work.read("fence.md"),
        "```text\n| a | b |\n| - | - |\n| スイープ | x |\n```\n"
    );
}

#[test]
fn realigned_tables_keep_crlf_line_ends() {
    let work = Work::new();
    work.write(
        "crlf.md",
        "| a | b |\r\n| - | - |\r\n| 掃引 | x |\r\n".as_bytes(),
    );
    ok(&work.transform(&["crlf.md"]));
    assert_eq!(
        work.read("crlf.md"),
        "| a        | b |\r\n| -------- | - |\r\n| スイープ | x |\r\n"
    );
}

#[test]
fn an_indented_table_to_realign_stops_without_writing() {
    // 字下げした表を揃え直す場面は確かめていないので、揃え直さずに止める
    let work = Work::new();
    work.write(
        "list.md",
        "- item\n\n  | a | b |\n  | - | - |\n  | 掃引 | x |\n".as_bytes(),
    );
    let out = work.transform(&["list.md"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("list.md") && err.contains("字下げ"), "{err}");
    assert!(work.read("list.md").contains("掃引"));
}

#[test]
fn a_table_row_with_an_escaped_pipe_stops_but_a_pipe_in_code_is_a_separator() {
    // 表の揃え方は \| でも列を切るので、揃え直さずに止める。コードスパンの中のエスケープしていない | は GFM でも区切り
    let work = Work::new();
    work.write(
        "escaped.md",
        "| a | b |\n| - | - |\n| x \\| y | 掃引 |\n".as_bytes(),
    );
    work.write(
        "code.md",
        "| a | b | c |\n| - | - | - |\n| `p|q` | 掃引 |\n".as_bytes(),
    );
    let out = work.transform(&["escaped.md"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("escaped.md") && err.contains("\\|"), "{err}");
    assert!(work.read("escaped.md").contains("掃引"));
    ok(&work.transform(&["code.md"]));
    assert_eq!(
        work.read("code.md"),
        "| a  | b  | c        |\n| -- | -- | -------- |\n| `p | q` | スイープ |\n"
    );
}

#[test]
fn a_missing_file_stops_before_writing_any_file() {
    let work = Work::new();
    work.write("doc.md", "確かめの記録\n".as_bytes());
    let out = work.transform(&["doc.md", "gone.md"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).starts_with("ERROR: "),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(work.read("doc.md"), "確かめの記録\n");
}

#[cfg(unix)]
#[test]
fn symlinks_are_skipped_and_paths_through_a_linked_directory_are_in_the_repository() {
    let work = Work::new();
    work.write("doc.md", "確かめの記録\n".as_bytes());
    std::os::unix::fs::symlink("doc.md", work.repo().join("link.md")).unwrap();
    let out = ok(&work.transform(&["link.md"]));
    assert_eq!(out, "SKIPPED: link.md (symlink)\nFILES: 0\n");
    assert_eq!(work.read("doc.md"), "確かめの記録\n");
    let alias = work.root.join("alias");
    std::os::unix::fs::symlink(work.repo(), &alias).unwrap();
    let out = ok(&work.transform(&[&alias.join("doc.md").display().to_string()]));
    assert_eq!(out, "REPLACED: doc.md\nFILES: 1\n");
}

#[test]
fn a_file_outside_the_repository_stops() {
    let work = Work::new();
    let outside = work.root.join("outside.md");
    std::fs::write(&outside, "確かめの記録\n").unwrap();
    let out = work.transform(&[&outside.display().to_string()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("の外"), "{}", text(&out.stderr));
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "確かめの記録\n");
}

#[test]
fn extra_rules_apply_with_the_dictionary_and_a_dictionary_replaces_the_default() {
    let work = Work::new();
    let extra = work.root.join("extra.yml");
    std::fs::write(
        &extra,
        "version: 1\nrules:\n  - expected: 新称\n    pattern: 旧称\n",
    )
    .unwrap();
    let extra = extra.display().to_string();
    work.write("a.md", "旧称の確かめの記録\n".as_bytes());
    ok(&work.transform(&["--extra-rules", &extra, "a.md"]));
    assert_eq!(work.read("a.md"), "新称の確認の記録\n");
    work.write("b.md", "旧称の確かめの記録\n".as_bytes());
    ok(&work.transform(&["--dictionary", &extra, "b.md"]));
    assert_eq!(work.read("b.md"), "新称の確かめの記録\n");
}

#[test]
fn a_dictionary_that_cannot_be_read_stops() {
    let work = Work::new();
    work.write("a.md", "確かめの記録\n".as_bytes());
    let missing = work.root.join("none.yml").display().to_string();
    let out = work.transform(&["--dictionary", &missing, "a.md"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("none.yml"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn usage_errors_exit_2() {
    let work = Work::new();
    assert_eq!(work.transform(&[]).status.code(), Some(2));
    assert_eq!(work.transform(&["--exclude"]).status.code(), Some(2));
    assert_eq!(
        work.transform(&["--unknown", "a.md"]).status.code(),
        Some(2)
    );
}
