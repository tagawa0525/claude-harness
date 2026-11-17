//! `claude-hooks script kept-sentences --old <元の版>... --new <今の版>...` が、縮めた規則の文書の文のうち、元の版に
//! 同じ文が無いものを出すことを確かめる（ADR-0088。`~/.claude/scripts/kept-sentences.sh` が呼ぶ）。sh から呼ぶ
//! 配備の確認は `claude/tests/scripts/kept-sentences.sh`

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Dir {
    root: PathBuf,
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "kept-sentences-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Dir { root }
    }

    fn write(&self, name: &str, text: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    }
}

/// 終了コードと標準出力
fn kept(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["script", "kept-sentences"])
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

fn compare(old: &str, new: &str) -> (i32, String, String) {
    let d = Dir::new();
    let old_path = d.write("old.md", old);
    let new_path = d.write("new.md", new);
    let (code, out) = kept(&["--old", &old_path, "--new", &new_path]);
    (code, out, new_path)
}

#[test]
fn the_same_version_prints_nothing_and_exits_0() {
    let text = "最初の文。最後の文。\n\n- 項目の文は折り返す。\n";
    let (code, out, _) = compare(text, text);
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn dropped_sentences_wrapping_list_marks_heading_levels_and_indent_are_not_counted() {
    let old = "## 見出し\n\n最初の文。消す文。最後の\n文。\n\n- 項目の文は\n  折り返す。\n";
    let new = "# 見出し\n\n最初の文。最後の文。\n\n項目の文は折り返す。\n";
    let (code, out, _) = compare(old, new);
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn a_dropped_word_is_printed_with_the_file_and_line() {
    let (code, out, path) = compare(
        "最初の文。最後の文。\n\n- 項目の文は折り返す。\n",
        "最初の文。最後の。\n\n- 項目の文は折り返す。\n",
    );
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {path}:1: 最後の。\n"));
}

#[test]
fn sentences_joined_across_paragraphs_are_found() {
    let (code, _, _) = compare(
        "前の段落の文\n\n次の段落の文。\n",
        "前の段落の文次の段落の文。\n",
    );
    assert_eq!(code, 1);
}

#[test]
fn sentences_moved_to_another_new_file_are_not_counted() {
    let d = Dir::new();
    let old = d.write("f.md", "- 一つ目の文。\n- 二つ目の文。\n");
    let first = d.write("f1.md", "- 一つ目の文。\n");
    let second = d.write("f2.md", "# 移した先\n\n- 二つ目の文。\n");
    let (code, out) = kept(&["--old", &old, "--new", &first, &second]);
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {second}:1: 移した先\n"));
}

#[test]
fn several_old_files_are_joined() {
    let d = Dir::new();
    let a = d.write("a.md", "一つ目の文。\n");
    let b = d.write("b.md", "二つ目の文。\n");
    let new = d.write("new.md", "一つ目の文。二つ目の文。\n");
    let (code, out) = kept(&["--old", &a, &b, "--new", &new]);
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn code_blocks_are_compared_line_by_line() {
    let (code, out, path) = compare(
        "```bash\njq -r . a.json\n```\n",
        "```bash\njq -r .x a.json\n```\n",
    );
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {path}:2: jq -r .x a.json\n"));
}

#[test]
fn table_rows_are_blocks_of_their_own() {
    let (code, out, path) = compare("前の文\n| a | b |\n", "前の文\n| a | c |\n");
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {path}:2: | a | c |\n"));
}

#[test]
fn quote_marks_are_removed() {
    let (code, out, _) = compare("> 引用の文。\n", "引用の文。\n");
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn a_dropped_condition_at_the_head_is_found_though_it_is_a_substring() {
    let (code, out, path) = compare("Aのときを除き、Bする。\n", "Bする。\n");
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {path}:1: Bする。\n"));
}

#[test]
fn a_dropped_limit_after_an_item_without_a_period_is_found() {
    let (code, _, _) = compare("- Xする（Yのときだけ）\n", "- Xする\n");
    assert_eq!(code, 1);
}

#[test]
fn periods_inside_brackets_do_not_end_a_sentence() {
    let (code, _, _) = compare(
        "Xする（理由。ただしYのときを除く）。\n",
        "Xする（理由。）。\n",
    );
    assert_eq!(code, 1);
}

#[test]
fn an_unpaired_closing_bracket_does_not_hide_later_sentence_ends() {
    let (code, out, path) = compare("1) 一つ目。二つ目。\n", "1) 一つ目。三つ目。\n");
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {path}:1: 三つ目。\n"));
}

#[test]
fn only_the_final_period_differs_when_prose_became_a_list_item() {
    let (code, out, _) = compare(
        "前の文。(b) 項目の文（理由）。\n",
        "- (b) 項目の文（理由）\n",
    );
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn removed_spaces_in_commands_and_changed_sentence_ends_are_found() {
    let (code, out, _) = compare(
        "`git log a..b` で見る。\nXするか？\n",
        "`gitlog a..b` で見る。\nXするか。\n",
    );
    assert_eq!(code, 1);
    assert!(out.contains("gitlog a..b"), "{out}");
    assert!(out.contains("Xするか。"), "{out}");
}

#[test]
fn wrapping_between_japanese_and_code_is_not_counted() {
    let (code, out, _) = compare(
        "記録は\n`git log a..b` で見る。\n",
        "記録は `git log\na..b` で見る。\n",
    );
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn missing_arguments_and_unreadable_files_exit_2() {
    let d = Dir::new();
    let a = d.write("a.md", "文。\n");
    assert_eq!(kept(&["--old", &a]).0, 2);
    assert_eq!(kept(&["--new", &a]).0, 2);
    assert_eq!(kept(&["--old", "--new", &a]).0, 2);
    let none = d.root.join("none.md").display().to_string();
    assert_eq!(kept(&["--old", &none, "--new", &a]).0, 2);
    assert_eq!(kept(&["--old", &a, "--new", &none]).0, 2);
}

#[test]
fn argparse_forms_of_the_options_are_accepted() {
    let d = Dir::new();
    let old = d.write("old.md", "一つ目の文。\n");
    let other = d.write("other.md", "二つ目の文。\n");
    let new = d.write("new.md", "一つ目の文。\n");
    // --old=FILE と、一意な省略形
    assert_eq!(
        kept(&[&format!("--old={old}"), "--new", &new]),
        (0, String::new())
    );
    assert_eq!(kept(&["--ol", &old, "--n", &new]), (0, String::new()));
    // 同じオプションを重ねると、後のものだけが残る
    let (code, out) = kept(&["--old", &old, "--old", &other, "--new", &new]);
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {new}:1: 一つ目の文。\n"));
}

#[test]
fn crlf_and_cr_line_ends_are_read_as_lf() {
    let (code, out, _) = compare(
        "最初の文。\n\n- 項目の\n  文。\n",
        "最初の文。\r\n\r\n- 項目の\r  文。\r\n",
    );
    assert_eq!((code, out.as_str()), (0, ""));
    // CR だけの改行も行の区切り（行の終わりの英数字の後には空白を補う）
    let (code, out, path) = compare("a\rb。\n", "x\rb。\n");
    assert_eq!(code, 1);
    assert_eq!(out, format!("CHANGED: {path}:1: x b。\n"));
}

#[test]
fn python_whitespace_controls_are_spaces() {
    let (code, out, _) = compare("- 項目の文。\n", "-\x1f項目の\x1c文。\n");
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn a_file_that_is_not_utf8_exits_2() {
    let d = Dir::new();
    let old = d.write("old.md", "文。\n");
    let bad = d.root.join("bad.md");
    std::fs::write(&bad, b"\xff\xfe\n").unwrap();
    assert_eq!(
        kept(&["--old", &old, "--new", &bad.display().to_string()]).0,
        2
    );
}

/// git の版を比べる元にするテストのリポジトリ（main に 1 つのコミット）
fn repo(files: &[(&str, &str)]) -> Dir {
    let d = Dir::new();
    for (name, text) in files {
        if let Some(parent) = Path::new(name).parent() {
            std::fs::create_dir_all(d.root.join(parent)).unwrap();
        }
        d.write(name, text);
    }
    let git = |args: &[&str]| {
        let ok = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&d.root)
            // 利用者のグローバルの設定（core.hooksPath のグローバルの git hook など）を読まない
            .env("GIT_CONFIG_GLOBAL", d.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "init"]);
    d
}

/// dir を cwd にして kept-sentences を呼ぶ
fn kept_in(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
        .args(["script", "kept-sentences"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn base_compares_the_version_at_the_base_with_the_working_tree() {
    let d = repo(&[("docs/a.md", "最初の文。最後の文。\n")]);
    assert_eq!(
        kept_in(&d.root, &["--base", "main", "docs/a.md"]),
        (0, String::new())
    );
    d.write("docs/a.md", "最初の文。最後の。\n");
    assert_eq!(
        kept_in(&d.root, &["--base", "main", "docs/a.md"]),
        (1, "CHANGED: docs/a.md:1: 最後の。\n".to_string())
    );
}

#[test]
fn base_reads_paths_relative_to_the_cwd_and_absolute_paths() {
    let d = repo(&[("docs/a.md", "最初の文。\n")]);
    d.write("docs/a.md", "別の文。\n");
    assert_eq!(
        kept_in(&d.root.join("docs"), &["--base", "main", "a.md"]),
        (1, "CHANGED: a.md:1: 別の文。\n".to_string())
    );
    // 絶対パスは cwd によらず、ファイルのあるリポジトリで base を引く（cwd は別のリポジトリの worktree でもよい）
    let abs = d.root.join("docs/a.md").display().to_string();
    let other = repo(&[("x.md", "x。\n")]);
    for cwd in [&d.root, &other.root] {
        assert_eq!(
            kept_in(cwd, &["--base", "main", &abs]),
            (1, format!("CHANGED: {abs}:1: 別の文。\n"))
        );
    }
}

#[test]
fn base_takes_a_new_file_as_a_destination_and_a_deleted_file_as_a_source() {
    // 文を移した先の新しいファイルは今の版だけ、移した元の消したファイルは元の版だけを比べる
    let d = repo(&[
        ("a.md", "- 一つ目の文。\n- 二つ目の文。\n"),
        ("c.md", "三つ目の文。\n"),
    ]);
    d.write("a.md", "- 一つ目の文。\n");
    d.write("b.md", "- 二つ目の文。\n- 三つ目の文。\n");
    std::fs::remove_file(d.root.join("c.md")).unwrap();
    assert_eq!(
        kept_in(&d.root, &["--base", "main", "a.md", "b.md", "c.md"]),
        (0, String::new())
    );
}

#[test]
fn base_errors_exit_2() {
    let d = repo(&[("a.md", "文。\n")]);
    // 解けない base、どちらの版にも無いパス、--old と --new との併用
    assert_eq!(kept_in(&d.root, &["--base", "no-such", "a.md"]).0, 2);
    assert_eq!(kept_in(&d.root, &["--base", "main", "none.md"]).0, 2);
    assert_eq!(kept_in(&d.root, &["--base", "main"]).0, 2);
    assert_eq!(
        kept_in(&d.root, &["--base", "main", "a.md", "--new", "a.md"]).0,
        2
    );
}

#[test]
fn base_takes_a_file_whose_directory_was_deleted_as_a_source() {
    // スキルのディレクトリごと消して、文を別のスキルに移した
    let d = repo(&[
        ("skills/foo/SKILL.md", "移す文。\n"),
        ("skills/bar/SKILL.md", "残る文。\n"),
    ]);
    std::fs::remove_dir_all(d.root.join("skills/foo")).unwrap();
    d.write("skills/bar/SKILL.md", "残る文。移す文。\n");
    assert_eq!(
        kept_in(
            &d.root,
            &[
                "--base",
                "main",
                "skills/foo/SKILL.md",
                "skills/bar/SKILL.md"
            ]
        ),
        (0, String::new())
    );
}
