//! `claude-hooks script fix-markdown-lint <file>...` が、markdownlint --fix の直せない MD040（コードの塊の言語）と
//! MD060（表の桁揃え）を直すことを確かめる（ADR-0087。`~/.claude/skills/language-checks/scripts/fix-markdown-lint.sh`
//! が呼ぶ）。sh から呼ぶ確認は `.claude/tests/scripts/language-checks.sh`

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
            "fix-markdown-lint-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Dir { root }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

fn fix(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["script", "fix-markdown-lint"])
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// text を 1 つのファイルに書いて直し、直した後の中身
fn fixed(text: &str) -> String {
    let dir = Dir::new();
    let path = dir.write("a.md", text.as_bytes());
    let out = fix(&[path.as_os_str()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_to_string(&path).unwrap()
}

#[test]
fn a_fixed_file_is_reported_as_fixed_and_a_clean_one_as_ok() {
    let dir = Dir::new();
    let bad = dir.write("bad.md", b"| a | b |\n| --- | --- |\n| ccc | d |\n");
    let good = dir.write("good.md", b"# ok\n");
    let out = fix(&[bad.as_os_str(), good.as_os_str()]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        format!("Fixed: {}\nOK: {}\n", bad.display(), good.display())
    );
}

#[test]
fn without_files_it_prints_the_usage_and_fails() {
    let out = fix(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("Usage:"), "{}", stdout(&out));
    // -- で始まる引数はファイルに数えない
    let out = fix(&["--fix".as_ref()]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_missing_file_is_skipped_silently() {
    let dir = Dir::new();
    let out = fix(&[dir.root.join("none.md").as_os_str()]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_file_that_is_not_utf8_fails_and_stops() {
    let dir = Dir::new();
    let bad = dir.write("bad.md", b"\xff\n");
    let after = dir.write("after.md", b"```\nfn main() {}\n```\n");
    let out = fix(&[bad.as_os_str(), after.as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        std::fs::read_to_string(&after).unwrap(),
        "```\nfn main() {}\n```\n"
    );
}

#[test]
fn crlf_is_read_as_lf_and_a_fixed_file_is_written_with_lf() {
    // .py の read_text は改行を \n に揃えて読む（universal newlines）
    assert_eq!(
        fixed("| a | b |\r\n| --- | --- |\r\n| ccc | d |\r\n"),
        "| a   | b |\n| --- | - |\n| ccc | d |\n"
    );
    // 直すものが無ければ書かないので、CRLF のまま
    assert_eq!(fixed("# a\r\n"), "# a\r\n");
}

#[test]
fn a_nested_fence_inside_a_longer_one_is_left_byte_for_byte() {
    let text = "````markdown\n```typescript\nconst x: string = \"a\";\n```\n````\n";
    assert_eq!(fixed(text), text);
}

#[test]
fn an_outer_fence_of_four_gets_a_language_and_keeps_its_close() {
    assert_eq!(
        fixed("````\nfn main() {}\n````\n"),
        "````rust\nfn main() {}\n````\n"
    );
}

#[test]
fn a_table_inside_a_tilde_fence_is_left_alone() {
    let text = "~~~text\n| a | b |\n| --- | --- |\n| ccc | d |\n~~~\n";
    assert_eq!(fixed(text), text);
}

#[test]
fn backticks_inside_a_tilde_fence_are_content() {
    assert_eq!(
        fixed("~~~\n```\nfn main() {}\n```\n~~~\n"),
        "~~~rust\n```\nfn main() {}\n```\n~~~\n"
    );
}

#[test]
fn a_backtick_fence_whose_info_has_a_backtick_is_not_a_fence() {
    let text = "```a`b\n| a | b |\n| --- | --- |\n| ccc | d |\n";
    assert_eq!(
        fixed(text),
        "```a`b\n| a   | b |\n| --- | - |\n| ccc | d |\n"
    );
}

#[test]
fn a_closing_fence_may_be_followed_by_python_whitespace() {
    // .py の \s は str.isspace で、\x1c〜\x1f を含む（Rust の char::is_whitespace は含まない）
    let text = "```text\n| a | b |\n| --- | --- |\n```\u{1c}\n| a | b |\n| --- | --- |\n";
    assert_eq!(
        fixed(text),
        "```text\n| a | b |\n| --- | --- |\n```\u{1c}\n| a | b |\n| - | - |\n"
    );
}

#[test]
fn tables_outside_fences_are_aligned_and_those_inside_are_not() {
    assert_eq!(
        fixed(
            "| a | b |\n| --- | --- |\n| ccc | d |\n\n```text\n| a | b |\n| --- | --- |\n| ccc | d |\n```\n"
        ),
        "| a   | b |\n| --- | - |\n| ccc | d |\n\n```text\n| a | b |\n| --- | --- |\n| ccc | d |\n```\n"
    );
}

#[test]
fn right_and_center_alignment_and_missing_cells_are_kept() {
    assert_eq!(
        fixed("| a | b | c |\n|---:|:---:|---|\n| long | x |\n"),
        "|    a | b | c |\n| ---: | :-: | - |\n| long | x |   |\n"
    );
}

#[test]
fn an_indented_table_keeps_its_spaces_and_tabs() {
    assert_eq!(
        fixed("1. step\n\n   | a | b |\n   | --- | --- |\n   | ccc | d |\n\n2. next\n"),
        "1. step\n\n   | a   | b |\n   | --- | - |\n   | ccc | d |\n\n2. next\n"
    );
    assert_eq!(
        fixed("- item\n\n\t| a | b |\n\t| --- | --- |\n\t| ccc | d |\n"),
        "- item\n\n\t| a   | b |\n\t| --- | - |\n\t| ccc | d |\n"
    );
    let aligned = "1. step\n\n   | a   | b |\n   | --- | - |\n   | ccc | d |\n";
    assert_eq!(fixed(aligned), aligned);
}

#[test]
fn a_leading_ideographic_space_is_not_kept_as_indent() {
    assert_eq!(
        fixed("\u{3000}| a | b |\n| --- | --- |\n| ccc | d |\n"),
        "| a   | b |\n| --- | - |\n| ccc | d |\n"
    );
}

#[test]
fn cells_are_stripped_of_python_whitespace() {
    // str.strip は \x1c〜\x1f も外す
    assert_eq!(
        fixed("| a | b |\n| --- | --- |\n| \u{1c}ccc | d |\n"),
        "| a   | b |\n| --- | - |\n| ccc | d |\n"
    );
}

/// ch だけのセルを幅 8 の列（どの場合より広い）に置いたときの、ch の幅（右の詰め物の数から逆算する）
fn width_of(ch: &str) -> usize {
    let out = fixed(&format!("| abcdefgh | x |\n| --- | --- |\n| {ch} | x |\n"));
    let row = out.lines().nth(2).unwrap();
    let rest = row
        .strip_prefix(&format!("| {ch}"))
        .unwrap_or_else(|| panic!("{row:?}"));
    let pad = rest
        .strip_suffix("| x |")
        .unwrap_or_else(|| panic!("{row:?}"));
    assert!(pad.chars().all(|c| c == ' '), "{row:?}");
    9 - pad.len()
}

#[test]
fn width_counts_fullwidth_and_wide_as_two_and_everything_else_as_one() {
    // 期待値は Python 3.14 の unicodedata 16.0.0 の East Asian Width で数えたもの。East Asian Width が F と W
    // だけを 2 に数え、幅の曖昧な文字（A）と、結合文字、制御文字、ゼロ幅の文字も 1 文字を 1 に数える。絵文字の列も
    // 1 文字ずつ足す
    let cases: &[(&str, usize)] = &[
        ("\u{2192}", 1),                   // → （A）
        ("\u{2605}", 1),                   // ★ （A）
        ("\u{00ad}", 1),                   // ソフトハイフン（A、Cf）
        ("\u{fe0f}", 1),                   // 異体字セレクタ 16（A、Mn）
        ("e\u{0301}", 2),                  // 結合のアキュート（A、Mn）
        ("\u{3099}", 2),                   // 結合の濁点（W、Mn）
        ("\u{304b}\u{3099}", 4),           // か + 結合の濁点
        ("\u{302a}", 2),                   // 漢字の声調の記号（W、Mn）
        ("\u{0007}", 1),                   // ベル（Cc）
        ("\u{001b}", 1),                   // ESC（Cc）
        ("\u{200b}", 1),                   // ゼロ幅の空白（Cf）
        ("\u{1f600}", 2),                  // 😀（W）
        ("\u{2764}", 1),                   // ❤（N）
        ("\u{2764}\u{fe0f}", 2),           // ❤️
        ("\u{1f468}\u{200d}\u{1f469}", 5), // ZWJ の列
        ("\u{1f1ef}", 1),                  // 地域の指示子（N）
        ("\u{1f3fb}", 2),                  // 肌の色の修飾子（W）
        ("\u{ff21}", 2),                   // 全角の Ａ（F）
        ("\u{ff71}", 1),                   // 半角のｱ（H）
        ("\u{6f22}", 2),                   // 漢（W）
        ("\u{1100}", 2),                   // ハングルの初声（W）
        ("\u{1160}", 1),                   // ハングルの中声（N）
    ];
    for (ch, want) in cases {
        assert_eq!(
            width_of(ch),
            *want,
            "{:?}",
            ch.chars().map(|c| c as u32).collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_fence_without_a_language_gets_one_guessed_from_its_content() {
    let cases: &[(&str, &str)] = &[
        ("{ pkgs, ... }: pkgs.hello", "nix"),
        ("fn main() {}", "rust"),
        ("def f():\n    pass", "python"),
        ("interface A { x: number }", "typescript"),
        ("const x = 1;", "javascript"),
        ("{\n  \"a\": 1\n}", "json"),
        ("name: value", "yaml"),
        ("[section]", "toml"),
        ("<a href=\"x\">y</a>", "html"),
        (".a { color: red }", "css"),
        ("SELECT a FROM t", "sql"),
        ("FROM alpine\nRUN true", "dockerfile"),
        ("git status", "bash"),
        ("hello", "text"),
    ];
    for (body, lang) in cases {
        assert_eq!(
            fixed(&format!("```\n{body}\n```\n")),
            format!("```{lang}\n{body}\n```\n"),
            "{body:?}"
        );
    }
}

#[test]
fn an_opening_fence_with_trailing_spaces_gets_the_language_after_them_are_stripped() {
    assert_eq!(
        fixed("```  \nfn main() {}\n```\n"),
        "```rust\nfn main() {}\n```\n"
    );
}
