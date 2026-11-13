//! `ja-terms message` と `ja-terms commit-msg` が、今の message.sh（ja-terms-message）と commit-msg-check.sh
//! （commit-msg-check）と同じ判定と終了コードを返すこと（ADR-0085）。
//!
//! 辞書の語を 1 行に書くと、このファイルが pre-commit の語の検査に掛かるので、崩れた語は分けて書いてつなぐ。

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// 崩れた語（辞書の最初の規則の、名詞の用法の語。直す語は「確認」）
const BROKEN: &str = concat!("確か", "め");

fn broken(text: &str) -> String {
    text.replace("{B}", BROKEN)
}

/// メッセージを標準入力で ja-terms message に渡す
fn message(text: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .arg("message")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// メッセージをファイルに書いて ja-terms commit-msg に渡す
fn commit_msg(name: &str, text: &str) -> Output {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("commit-msg");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .arg("commit-msg")
        .arg(&path)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 崩れた語を報告して 1 で終わったか（それ以外の終わり方は理由つきで落とす）
fn stops(out: &Output) -> bool {
    match out.status.code() {
        Some(0) => false,
        Some(1) => true,
        code => panic!(
            "{code:?}: {}{}",
            stdout(out),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

// ja-terms message

#[test]
fn a_broken_term_in_the_message_is_reported_with_the_fix() {
    let out = message(&broken("docs: {B}の結果を書く\n"));
    assert!(stops(&out));
    assert_eq!(stdout(&out), format!("1:7: {BROKEN} => 確認\n"));
}

#[test]
fn positions_are_lines_and_columns_of_the_checked_body() {
    let out = message(&broken("docs: add\n\n本文の\n二行目の{B}の結果\n"));
    assert_eq!(stdout(&out), format!("4:5: {BROKEN} => 確認\n"));
}

#[test]
fn the_body_is_read_as_markdown() {
    let cases = [
        (false, "`{B}の結果` を書く"),
        (true, "`foo` と{B}の結果を書く"),
        (false, "> {B}の結果を書く"),
        (false, "[{B}の結果を書く](http://example.com)"),
        (false, "*{B}の結果*を書く"),
        (false, "_{B}の結果_ を書く"),
        (false, "```\n{B}の結果\n```"),
        (true, "## {B}の結果"),
        (true, "- {B}の結果を書く"),
    ];
    for (stop, body) in cases {
        let text = broken(&format!("docs: add\n\n本文\n\n{body}\n"));
        assert_eq!(stops(&message(&text)), stop, "{body}");
    }
}

#[test]
fn subjects_copied_or_made_by_git_are_left_out() {
    for subject in [
        "Merge branch x",
        "fixup! docs: {B}の結果",
        "squash! docs: {B}の結果",
        "Revert \"docs: {B}の結果\"",
    ] {
        let out = message(&broken(&format!("{subject}\n")));
        assert!(!stops(&out), "{subject}");
        assert_eq!(stdout(&out), "", "{subject}");
    }
}

#[test]
fn the_body_after_a_left_out_subject_is_still_checked() {
    let out = message(&broken("squash! docs: add\n\n{B}の結果を書く\n"));
    assert!(stops(&out));
    // 件名の行を除いた本文の位置
    assert_eq!(stdout(&out), format!("2:1: {BROKEN} => 確認\n"));
}

#[test]
fn the_subject_is_the_first_line_not_starting_with_a_hash() {
    // 前の # の行を飛ばした最初の行が件名で、その行だけを除く
    let out = message(&broken("# 注記\nfixup! docs: {B}の結果\n"));
    assert!(!stops(&out));
    let out = message(&broken("# {B}の結果\nfixup! docs: add\n"));
    assert!(stops(&out));
}

#[test]
fn lines_starting_with_a_hash_are_checked() {
    // -m や -F（cleanup=whitespace）では消えずにコミットに残る
    let out = message(&broken("docs: add\n\n#290 の{B}の結果\n"));
    assert!(stops(&out));
}

#[test]
fn the_diff_after_the_scissors_line_is_left_out() {
    let scissors = format!("# {} >8 {}", "-".repeat(24), "-".repeat(24));
    let out = message(&broken(&format!(
        "docs: add\n\n本文\n{scissors}\n{BROKEN}の差分\n"
    )));
    assert!(!stops(&out));
}

#[test]
fn an_empty_or_blank_body_is_not_checked() {
    for text in ["", "\n", "fixup! docs: add\n", "Merge branch x\n\n  \n\n"] {
        let out = message(text);
        assert_eq!(out.status.code(), Some(0), "{text:?}");
        assert_eq!(stdout(&out), "", "{text:?}");
    }
}

// ja-terms commit-msg

#[test]
fn conventional_subjects_pass_and_others_stop() {
    for (ok, subject) in [
        (true, "feat(scope): 追加する"),
        (true, "chore(flake): update (testhost)"),
        (true, "fix!: break it"),
        (false, "flake: update (testhost)"),
        (false, "何の型もない件名"),
        (false, "feat:  two spaces"),
        (false, "Add the proposed request"),
    ] {
        let out = commit_msg("subject", &format!("{subject}\n"));
        assert_eq!(out.status.code(), Some(if ok { 0 } else { 1 }), "{subject}");
        if !ok {
            assert_eq!(
                stdout(&out),
                format!(
                    "❌ Conventional Commits 形式ではありません: {subject}\n   形式: <type>(<scope>)?: <subject>    type: feat|fix|docs|style|refactor|test|chore|perf|build|ci|revert\n"
                ),
                "{subject}"
            );
        }
    }
}

#[test]
fn subjects_are_limited_to_72_characters() {
    for (ok, subject) in [
        (true, format!("feat: {}", "a".repeat(66))),
        (false, format!("feat: {}", "a".repeat(67))),
        (true, format!("docs: {}", "あ".repeat(66))),
        (false, format!("docs: {}", "あ".repeat(67))),
    ] {
        let out = commit_msg("length", &format!("{subject}\n"));
        assert_eq!(out.status.code(), Some(if ok { 0 } else { 1 }), "{subject}");
        if !ok {
            assert_eq!(
                stdout(&out),
                format!("❌ 件名が 72 文字を超えています (73 文字): {subject}\n")
            );
        }
    }
}

#[test]
fn subjects_over_50_characters_are_warned() {
    let subject = format!("feat: {}", "a".repeat(45));
    let out = commit_msg("warn", &format!("{subject}\n"));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        "⚠️  件名が 50 文字を超えています (51 文字)。短くできないか検討してください\n"
    );
}

#[test]
fn subjects_copied_or_made_by_git_skip_the_form_and_length() {
    let long = "a".repeat(80);
    for subject in [
        format!("Merge: {long}"),
        format!("fixup! {long}"),
        format!("squash! {long}"),
        format!("Revert \"{long}\""),
    ] {
        let out = commit_msg("exempt", &format!("{subject}\n"));
        assert_eq!(out.status.code(), Some(0), "{subject}");
        assert_eq!(stdout(&out), "", "{subject}");
    }
}

#[test]
fn broken_terms_in_the_message_stop_with_the_fix() {
    let out = commit_msg("terms", &broken("docs: {B}の結果を書く\n"));
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stdout(&out),
        format!(
            "1:7: {BROKEN} => 確認\n❌ コミットの本文に崩れた日本語の語があります。上の「誤り => 正しい語」に直してください\n"
        )
    );
}

#[test]
fn broken_terms_in_the_body_of_exempt_subjects_stop() {
    let out = commit_msg("exempt-terms", &broken("Merge branch x\n\n{B}の結果\n"));
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn the_form_is_checked_before_the_terms() {
    let out = commit_msg("order", &broken("{B}の結果\n"));
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).starts_with("❌ Conventional Commits 形式ではありません"));
}

#[test]
fn a_missing_message_file_is_a_failure() {
    let out = Command::new(env!("CARGO_BIN_EXE_ja-terms"))
        .args(["commit-msg", "/nonexistent/COMMIT_EDITMSG"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn nul_bytes_are_dropped_from_the_subject_as_from_the_body() {
    // commit-msg-check.sh は件名を sh の変数に取り、NUL は落ちる（本文の切り出しと同じ）
    let out = commit_msg("nul", "fix\0: x\n");
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
}
