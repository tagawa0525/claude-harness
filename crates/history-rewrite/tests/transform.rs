//! `history-rewrite transform [語の指定] FILE...` が、作業ツリーのファイルの語を、run のステージ 1 と同じ変換で直すこと
//! （ADR-0045、ADR-0088）。既定の辞書は ja-terms の共通の辞書（テストは ja-terms の crate の辞書を渡す。ADR-0097）。
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

/// ja-terms の共通の辞書（ビルドに埋めず、ホームの下か、この環境変数のパスから読む。ADR-0097）。テストは ja-terms の
/// crate の辞書を渡す
const COMMON_DICTIONARY: (&str, &str) = (
    ja_terms::common::COMMON_DICTIONARY_ENV,
    concat!(env!("CARGO_MANIFEST_DIR"), "/../ja-terms/prh.yml"),
);

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
            .env(COMMON_DICTIONARY.0, COMMON_DICTIONARY.1)
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
fn an_indented_table_in_a_list_item_is_realigned_keeping_its_indent() {
    let work = Work::new();
    work.write(
        "list.md",
        "- item\n\n  | a | b |\n  | - | - |\n  | 掃引 | x |\n".as_bytes(),
    );
    ok(&work.transform(&["list.md"]));
    assert_eq!(
        work.read("list.md"),
        "- item\n\n  | a        | b |\n  | -------- | - |\n  | スイープ | x |\n"
    );
}

#[test]
fn a_table_shaped_row_in_an_indented_code_block_is_left_alone() {
    let work = Work::new();
    let src = "text\n\n    | a | b |\n    | - | - |\n    | 掃引 | x |\n";
    work.write("code.md", src.as_bytes());
    ok(&work.transform(&["code.md"]));
    assert_eq!(work.read("code.md"), src.replace("掃引", "スイープ"));
}

#[test]
fn a_table_shaped_code_block_in_a_list_item_is_left_alone() {
    // 項の中身の桁（3）から 4 桁以上の字下げはコードブロック（#347）
    let work = Work::new();
    let src = "1. item\n\n        | a | b |\n        | - | - |\n        | 掃引 | x |\n";
    work.write("code.md", src.as_bytes());
    ok(&work.transform(&["code.md"]));
    assert_eq!(work.read("code.md"), src.replace("掃引", "スイープ"));
}

#[test]
fn a_table_in_a_list_item_opened_by_a_marker_only_line_is_realigned_keeping_crlf() {
    // 印だけの行（`-\r\n`）の次の行が中身なら項の中で、中身の桁（2）から 2 桁の字下げの表
    let work = Work::new();
    work.write(
        "crlf.md",
        "-\r\n  text\r\n\r\n    | a | b |\r\n    | - | - |\r\n    | 掃引 | x |\r\n".as_bytes(),
    );
    ok(&work.transform(&["crlf.md"]));
    assert_eq!(
        work.read("crlf.md"),
        "-\r\n  text\r\n\r\n    | a        | b |\r\n    | -------- | - |\r\n    | スイープ | x |\r\n"
    );
}

#[test]
fn an_escaped_pipe_stays_in_its_cell_but_a_pipe_in_code_is_a_separator() {
    // \| は列を切らない。コードスパンの中のエスケープしていない | は GFM でも区切り
    let work = Work::new();
    work.write(
        "escaped.md",
        "| a | b |\n| - | - |\n| x \\| y | 掃引 |\n".as_bytes(),
    );
    work.write(
        "code.md",
        "| a | b | c |\n| - | - | - |\n| `p|q` | 掃引 |\n".as_bytes(),
    );
    ok(&work.transform(&["escaped.md"]));
    assert_eq!(
        work.read("escaped.md"),
        "| a      | b        |\n| ------ | -------- |\n| x \\| y | スイープ |\n"
    );
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

#[test]
fn options_also_take_their_value_after_an_equals_sign() {
    let work = Work::new();
    work.write("dict/terms.yml", "pattern: 確かめの\n".as_bytes());
    let out = ok(&work.transform(&["--exclude=**/terms.yml", "dict/terms.yml"]));
    assert_eq!(out, "SKIPPED: dict/terms.yml (excluded)\nFILES: 0\n");
}

#[test]
fn a_replacement_that_changes_the_number_of_lines_in_markdown_stops() {
    // 表の揃え直しは行の対応で直した表を決めるので、行の数を変える置き換えは止める
    let work = Work::new();
    let rules = work.root.join("newline.yml");
    std::fs::write(
        &rules,
        "version: 1\nrules:\n  - expected: \"一\\n二\"\n    pattern: 一二\n",
    )
    .unwrap();
    let rules = rules.display().to_string();
    work.write("a.md", "一二\n".as_bytes());
    let out = work.transform(&["--dictionary", &rules, "a.md"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("行の数が変わる"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(work.read("a.md"), "一二\n");
}

// リポジトリの辞書（作業ツリーの直下の .ja-terms/prh.yml。ADR-0096）

const REPO_RULES: &str = "version: 1\nrules:\n  - expected: 新称\n    pattern: 旧称\n";

#[test]
fn the_repository_dictionary_is_layered_over_the_common_one() {
    let work = Work::new();
    work.write(".ja-terms/prh.yml", REPO_RULES.as_bytes());
    work.write("doc.md", "確かめの旧称\n".as_bytes());
    ok(&work.transform(&["doc.md"]));
    assert_eq!(work.read("doc.md"), "確認の新称\n");
}

#[test]
fn a_repository_rule_with_the_same_expected_replaces_the_common_rule() {
    // prh の Engine.merge（imports）と同じく、共通の辞書の同じ expected の規則を外す
    let work = Work::new();
    work.write(
        ".ja-terms/prh.yml",
        "version: 1\nrules:\n  - expected: 確認\n    pattern: 旧称\n".as_bytes(),
    );
    work.write("doc.md", "確かめの旧称\n".as_bytes());
    ok(&work.transform(&["doc.md"]));
    assert_eq!(work.read("doc.md"), "確かめの確認\n");
}

#[test]
fn an_explicit_dictionary_is_not_layered_with_the_repository_dictionary() {
    let work = Work::new();
    work.write(".ja-terms/prh.yml", REPO_RULES.as_bytes());
    work.write("doc.md", "確かめの旧称\n".as_bytes());
    let given = work.root.join("given.yml");
    std::fs::write(
        &given,
        "version: 1\nrules:\n  - expected: 確認\n    pattern: 確かめ\n",
    )
    .unwrap();
    ok(&work.transform(&["--dictionary", given.to_str().unwrap(), "doc.md"]));
    assert_eq!(work.read("doc.md"), "確認の旧称\n");
}

#[test]
fn an_explicit_repository_dictionary_is_layered_over_the_common_one() {
    // 作業ツリーの外のファイルでも、--repo-dictionary で渡せば重ねる
    let work = Work::new();
    let given = work.root.join("repo.yml");
    std::fs::write(&given, REPO_RULES).unwrap();
    work.write("doc.md", "確かめの旧称\n".as_bytes());
    ok(&work.transform(&["--repo-dictionary", given.to_str().unwrap(), "doc.md"]));
    assert_eq!(work.read("doc.md"), "確認の新称\n");
}

#[test]
fn an_unreadable_repository_dictionary_stops_the_transform() {
    let work = Work::new();
    work.write(".ja-terms/prh.yml", b"version: 1\nrules: [\n");
    work.write("doc.md", "確かめの旧称\n".as_bytes());
    let out = work.transform(&["doc.md"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    assert!(
        text(&out.stderr).contains(".ja-terms/prh.yml"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(work.read("doc.md"), "確かめの旧称\n");
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_repository_dictionary_stops_the_transform() {
    // 行き先の無い symlink は「辞書が無い」でなく、読めない辞書として止める
    let work = Work::new();
    std::fs::create_dir_all(work.repo().join(".ja-terms")).unwrap();
    std::os::unix::fs::symlink("missing.yml", work.repo().join(".ja-terms/prh.yml")).unwrap();
    work.write("doc.md", "確かめの旧称\n".as_bytes());
    let out = work.transform(&["doc.md"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    assert!(
        text(&out.stderr).contains(".ja-terms/prh.yml"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(work.read("doc.md"), "確かめの旧称\n");
}
