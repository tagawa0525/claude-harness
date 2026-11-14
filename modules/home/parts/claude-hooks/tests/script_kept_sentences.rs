//! `claude-hooks script kept-sentences --old <元の版>... --new <今の版>...` が、縮めた規則の文書の文のうち、元の版に
//! 同じ文が無いものを出すことを確かめる（ADR-0088。`~/.claude/scripts/kept-sentences.sh` が呼ぶ）。sh から呼ぶ
//! 配備の確認は `.claude/tests/scripts/kept-sentences.sh`

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
