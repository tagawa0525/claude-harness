//! `ja-terms check` が、textlint 15.7.1 と textlint-rule-prh 6.0.0 の `--format unix` と同じ報告を出すこと（ADR-0085）。
//!
//! 期待値（tests/textlint/<入力>.unix）は tests/textlint/update.sh が、tests/textlint/report.yml を当てた textlint で作る。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/textlint")
}

/// 検査に渡す名前。固定の入力は markdownlint が直さないよう .markdown にしてあるので、Markdown は .md にして渡す
/// （ja-terms-files と同じく、Markdown の拡張子は md だけ）。絶対パス（scratch のファイル）はそのまま渡す
fn input_name(name: &str) -> String {
    match name.strip_suffix(".markdown") {
        Some(stem) if Path::new(name).is_relative() => format!("{stem}.md"),
        _ => name.to_string(),
    }
}

/// 固定の入力（相対の名前）を一時のディレクトリにコピーし、そこで report.yml を辞書にして検査する。テストは並行に
/// 走るので、呼ぶたびに別のディレクトリにする（同じ名前にコピーし直す間に、ほかのテストが読まないよう）
fn check(files: &[&str]) -> Output {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("inputs")
        .join(NEXT.fetch_add(1, Ordering::SeqCst).to_string());
    std::fs::create_dir_all(&dir).unwrap();
    for name in files.iter().filter(|name| Path::new(name).is_relative()) {
        let source = fixtures().join(name);
        if source.exists() {
            std::fs::copy(&source, dir.join(input_name(name))).unwrap();
        }
    }
    Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .current_dir(&dir)
        .arg("check")
        .arg("--dictionary")
        .arg(fixtures().join("report.yml"))
        .args(files.iter().map(|name| input_name(name)))
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// textlint の報告の、ファイルの名前を検査に渡す名前にしたもの
fn expected(name: &str) -> String {
    let report = std::fs::read_to_string(fixtures().join(format!("{name}.unix"))).unwrap();
    report.replace(&format!("{name}:"), &format!("{}:", input_name(name)))
}

/// 1 つのファイルの報告の行（末尾の件数の行と、その前の空行を除く）
fn findings(report: &str) -> &str {
    let end = report.rfind("\n\n").map_or(report.len(), |at| at + 1);
    &report[..end]
}

/// 一時のファイルを書いて、そのパスを返す
fn scratch(name: &str, content: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("check");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn assert_reports_as_textlint(name: &str) {
    let out = check(&[name]);
    assert_eq!(stdout(&out), expected(name), "{name}");
    assert_eq!(out.status.code(), Some(1), "{name}");
}

#[test]
fn text_files_are_reported_as_textlint_does() {
    assert_reports_as_textlint("report.txt");
}

#[test]
fn markdown_is_reported_as_textlint_does() {
    assert_reports_as_textlint("report.markdown");
}

#[test]
fn positions_after_a_byte_order_mark_and_crlf_are_reported_as_textlint_does() {
    assert_reports_as_textlint("report-bom.markdown");
}

#[test]
fn files_are_reported_in_the_given_order_with_one_total() {
    let out = check(&["report.txt", "report.markdown"]);
    let txt = expected("report.txt");
    let md = expected("report.markdown");
    let total = |r: &str| findings(r).lines().count();
    assert_eq!(
        stdout(&out),
        format!(
            "{}{}\n{} problems\n",
            findings(&txt),
            findings(&md),
            total(&txt) + total(&md)
        )
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_single_finding_is_counted_as_one_problem() {
    let path = scratch("one.txt", "前の 確忍 の後\n");
    let out = check(&[path.to_str().unwrap()]);
    assert_eq!(
        stdout(&out),
        format!(
            "{}:1:4: 確忍 => 確認 [Error/prh]\n\n1 problem\n",
            path.display()
        )
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn files_without_findings_print_nothing_and_exit_0() {
    let out = check(&["lines.txt", "blocks.markdown"]);
    assert_eq!(stdout(&out), "");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn extensions_are_matched_without_case() {
    let path = scratch("upper.MD", "# 確忍\n");
    let out = check(&[path.to_str().unwrap()]);
    assert_eq!(
        stdout(&out),
        format!(
            "{}:1:3: 確忍 => 確認 [Error/prh]\n\n1 problem\n",
            path.display()
        )
    );
}

#[test]
fn code_files_are_read_line_by_line() {
    // Markdown なら `確忍` はコードスパンで見ない
    let path = scratch("code.rs", "// `確忍`\n");
    let out = check(&[path.to_str().unwrap()]);
    assert_eq!(
        stdout(&out),
        format!(
            "{}:1:5: 確忍 => 確認 [Error/prh]\n\n1 problem\n",
            path.display()
        )
    );
}

#[test]
fn files_of_other_extensions_are_refused_with_exit_2() {
    for name in ["notes.html", "Makefile", "notes.markdown"] {
        let path = scratch(name, "確忍\n");
        let out = check(&[path.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(2), "{name}");
        assert_eq!(stdout(&out), "", "{name}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(path.to_str().unwrap()), "{name}: {err}");
    }
}

#[test]
fn unreadable_files_are_refused_with_exit_2() {
    let out = check(&["missing.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing.txt"));
}

#[test]
fn no_files_is_a_usage_error() {
    let out = check(&[]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_broken_dictionary_is_refused_with_exit_2() {
    let dictionary = scratch("broken.yml", "version: 2\nrules: []\n");
    let out = Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .current_dir(fixtures())
        .args(["check", "--dictionary", dictionary.to_str().unwrap()])
        .arg("lines.txt")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn the_ja_terms_dictionary_is_used_by_default() {
    // 辞書の語を 1 行に書くと、このファイルが pre-commit の語の検査に掛かる
    let path = scratch("default.md", concat!("確か", "めの結果\n"));
    let out = Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(
        stdout(&out),
        format!(
            "{}:1:1: {} => 確認 [Error/prh]\n\n1 problem\n",
            path.display(),
            concat!("確か", "め")
        )
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_closed_output_still_exits_1_when_terms_are_found() {
    // 報告を読み終える前に閉じる読み手（`| head -1` など）でも、判定は崩れた語があること
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .current_dir(fixtures())
        .args(["check", "--dictionary", "report.yml", "report.txt"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn names_starting_with_a_dot_are_read_by_the_extension_after_it() {
    // ja-terms-files と同じく、`.md` の拡張子は md
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("dot");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".md");
    std::fs::write(&path, "# 確忍\n").unwrap();
    let out = check(&[path.to_str().unwrap()]);
    assert_eq!(
        stdout(&out),
        format!(
            "{}:1:3: 確忍 => 確認 [Error/prh]\n\n1 problem\n",
            path.display()
        )
    );
}

#[test]
fn a_file_given_twice_is_checked_once() {
    // textlint は引数をファイルの集まりにするので、同じファイルは 1 度だけ報告する
    let path = scratch("twice.txt", "確忍\n");
    let dot = path.parent().unwrap().join(".").join("twice.txt");
    let out = check(&[path.to_str().unwrap(), dot.to_str().unwrap()]);
    assert_eq!(
        stdout(&out),
        format!(
            "{}:1:1: 確忍 => 確認 [Error/prh]\n\n1 problem\n",
            path.display()
        )
    );
}
