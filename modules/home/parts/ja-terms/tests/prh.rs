//! 辞書を prh 6.0.0 と同じ意味で読み、文に当てること（ADR-0085）。
//!
//! 辞書の語（崩れた語）をわざと含むので、.textlintignore で ja-terms の検査から外す。

use ja_terms::{Dictionary, Diff};

fn load(yaml: &str) -> Dictionary {
    Dictionary::parse(yaml, "test.yml").unwrap_or_else(|e| panic!("{e}"))
}

fn rule(expected: &str, pattern: &str) -> Dictionary {
    load(&format!(
        "version: 1\nrules:\n  - expected: '{expected}'\n    pattern: '{pattern}'\n"
    ))
}

fn apply(dictionary: &Dictionary, text: &str) -> String {
    dictionary.apply(text).unwrap_or_else(|e| panic!("{e}"))
}

fn refused(yaml: &str) -> String {
    match Dictionary::parse(yaml, "test.yml") {
        Ok(_) => panic!("読めてしまった: {yaml}"),
        Err(e) => e.to_string(),
    }
}

fn refused_rule(pattern: &str) -> String {
    refused(&format!(
        "version: 1\nrules:\n  - expected: X\n    pattern: '{pattern}'\n"
    ))
}

// 辞書

#[test]
fn loads_the_ja_terms_dictionary_and_its_specs_pass() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/prh.yml");
    let dictionary = Dictionary::load(path).unwrap_or_else(|e| panic!("{e}"));
    assert!(dictionary.len() > 100);
    assert_eq!(
        apply(&dictionary, "確かめの結果を確かめる"),
        "確認の結果を確かめる"
    );
}

#[test]
fn a_dictionary_with_crlf_line_ends_reads_as_with_lf() {
    // Git for Windows は core.autocrlf=true で辞書を CRLF で取り出す
    let yaml = "version: 1\r\nrules:\r\n  - expected: X\r\n    pattern: |\r\n      /foo/\r\n";
    assert_eq!(apply(&load(yaml), "foo /foo/\n"), "foo X");
}

#[test]
fn a_missing_file_is_refused_with_its_path() {
    let e = Dictionary::load("/nonexistent/prh.yml").unwrap_err();
    assert!(e.to_string().contains("/nonexistent/prh.yml"), "{e}");
}

// pattern は JavaScript の正規表現（u と m のフラグ）として読む

#[test]
fn variable_width_lookbehind_with_line_start_reads_like_javascript() {
    let d = rule("漏れ", "/(?<=[にがの、]|^)抜け(?=[をの])/");
    assert_eq!(apply(&d, "判定に抜けを\n抜けの"), "判定に漏れを\n漏れの");
    assert_eq!(apply(&d, "すり抜けを"), "すり抜けを");
    let d = rule("比較", "/(?<!走らせ|走り)比べ/");
    assert_eq!(apply(&d, "走らせ比べ と比べ"), "走らせ比べ と比較");
}

#[test]
fn line_start_follows_carriage_return_as_in_javascript() {
    assert_eq!(apply(&rule("X", "/^a/"), "b\ra"), "b\rX");
}

#[test]
fn dot_does_not_match_line_terminators() {
    assert_eq!(apply(&rule("X", "/a.b/"), "a\rb a-b"), "a\rb X");
}

#[test]
fn lookahead_may_see_a_newline_inside_the_paragraph() {
    let d = rule("ラッパー", "/包み(?!\\n?込)/");
    assert_eq!(apply(&d, "包み\n込む 包みを"), "包み\n込む ラッパーを");
}

#[test]
fn class_shorthands_match_what_javascript_matches() {
    // JavaScript の \s は全角の空白を含み、\d と \w は ASCII だけ
    assert_eq!(apply(&rule("X", "/a\\sb/"), "a\u{3000}b"), "X");
    assert_eq!(apply(&rule("X", "/\\d/"), "１1"), "１X");
    assert_eq!(apply(&rule("X", "/\\w/"), "あa"), "あX");
}

#[test]
fn unicode_escapes_and_groups_read_under_the_u_flag() {
    assert_eq!(apply(&rule("X", "/\\p{Script=Han}/"), "あ漢"), "あX");
    assert_eq!(apply(&rule("X", "/(?<n>a)\\k<n>/"), "a aa"), "a X");
    assert_eq!(apply(&rule("X", "/\\u{1F600}/"), "😀"), "X");
    assert_eq!(apply(&rule("X", "/a[^]b/"), "a\nb"), "X");
}

#[test]
fn fixed_string_pattern_matches_literally() {
    assert_eq!(apply(&rule("X", "a.b"), "a.b axb"), "X axb");
    assert_eq!(apply(&rule("X", "(a)"), "(a) a"), "X a");
}

#[test]
fn a_pattern_with_a_trailing_newline_is_a_fixed_string() {
    // prh の /^\/(.*)\/([gimy]*)$/ は、末尾の改行の前の / を終わりとみなさない
    let d = load("version: 1\nrules:\n  - expected: X\n    pattern: |\n      /foo/\n");
    assert_eq!(apply(&d, "foo /foo/\n"), "foo X");
}

#[test]
fn patterns_list_matches_any_of_them() {
    let d =
        load("version: 1\nrules:\n  - expected: スイープ\n    patterns: [掃引, /スウィープ/]\n");
    assert_eq!(apply(&d, "掃引とスウィープ"), "スイープとスイープ");
}

// 直し方

#[test]
fn dollar_groups_are_substituted_and_out_of_range_ones_stay() {
    assert_eq!(apply(&rule("$1-$2-$0-$5", "/(a)(b)?c/"), "ac"), "a--$0-$5");
}

#[test]
fn a_two_digit_dollar_beyond_the_groups_stays_whole() {
    // diff.js の /\$([0-9]{1,2})/ は 2 桁まで読み、群の数を超えればそのまま残す
    assert_eq!(apply(&rule("$12", "/(a)/"), "a"), "$12");
}

#[test]
fn a_match_equal_to_expected_is_left_alone() {
    let d = rule("ウェブ", "/ウ[ェエ]ブ/");
    assert_eq!(apply(&d, "ウェブとウエブ"), "ウェブとウェブ");
}

#[test]
fn all_rules_match_the_original_text_not_each_others_output() {
    let d = load(
        "version: 1\nrules:\n  - expected: b\n    pattern: a\n  - expected: c\n    pattern: b\n",
    );
    assert_eq!(apply(&d, "ab"), "bc");
}

#[test]
fn overlaps_keep_the_longer_at_the_same_start_and_the_earlier_otherwise() {
    let d = load(
        "version: 1\nrules:\n  - expected: S\n    pattern: ab\n  - expected: L\n    pattern: abc\n  - expected: Z\n    pattern: cd\n",
    );
    assert_eq!(apply(&d, "abcd"), "Ld");
    assert_eq!(apply(&d, "xbcd ab"), "xbZ S");
}

#[test]
fn prh_disable_is_read_as_plain_text() {
    let d = rule("確認", "確かめ");
    assert_eq!(
        apply(&d, "`prh:disable` の確かめの"),
        "`prh:disable` の確認の"
    );
}

#[test]
fn lookbehind_does_not_see_the_previous_paragraph() {
    let d = rule("計数", "/(?<=の)数え/");
    assert_eq!(apply(&d, "の\n\n数えが"), "の\n\n数えが");
    assert_eq!(apply(&d, "の\n\n件の数えが"), "の\n\n件の計数が");
}

#[test]
fn diffs_are_byte_ranges_of_the_whole_text_in_order() {
    let d = load(
        "version: 1\nrules:\n  - expected: 確認\n    pattern: 確かめ\n  - expected: X\n    pattern: a\n",
    );
    assert_eq!(
        d.diffs("a\n\nの確かめ").unwrap(),
        vec![
            Diff {
                start: 0,
                end: 1,
                replacement: "X".into()
            },
            Diff {
                start: 6,
                end: 15,
                replacement: "確認".into()
            },
        ]
    );
}

#[test]
fn empty_matches_are_refused() {
    assert!(rule("X", "/a*/").apply("b").is_err());
}

// specs

#[test]
fn specs_are_checked_on_the_whole_string_like_rule_check() {
    // prh の Rule.check は spec.from を段落に分けずに applyRule に渡す
    let d = load(
        "version: 1\nrules:\n  - expected: C\n    pattern: /A\\n\\nB/\n    specs:\n      - from: \"A\\n\\nB\"\n        to: C\n",
    );
    assert_eq!(d.len(), 1);
}

#[test]
fn a_failing_spec_stops_loading() {
    let e = refused(
        "version: 1\nrules:\n  - expected: 確認\n    pattern: 確かめ\n    specs:\n      - from: 確かめの\n        to: 確かめの\n",
    );
    assert!(e.contains("specs"), "{e}");
    assert!(e.contains("1 番目の規則"), "{e}");
}

#[test]
fn spec_values_that_are_not_strings_are_refused() {
    // js-yaml（YAML 1.2）は 1:30 を文字列と、90 を数と読む。prh は数を文字列として当てないので、引用符で囲む
    let e = refused(
        "version: 1\nrules:\n  - expected: X\n    pattern: \"90\"\n    specs:\n      - from: 90\n        to: X\n",
    );
    assert!(e.contains("specs の from と to は文字列"), "{e}");
    load(
        "version: 1\nrules:\n  - expected: X\n    pattern: \"1:30\"\n    specs:\n      - from: 1:30\n        to: X\n",
    );
}

// prh が読めない書き方

#[test]
fn syntax_javascript_refuses_under_the_u_flag_is_refused() {
    // prh は固定の文字列の - と ¥ を \- と \¥ にし、u フラグの RegExp では Invalid escape になる
    for pattern in [
        "e-mail", "¥100", "/a{,3}/", "/x{/", "/x}/", "/x]/", "/a\\-b/", "/(a/",
    ] {
        let e = refused_rule(pattern);
        assert!(e.contains("1 番目の規則"), "{pattern}: {e}");
    }
}

#[test]
fn ignore_case_flag_is_refused() {
    let e = refused_rule("/a/i");
    assert!(e.contains("i フラグ"), "{e}");
}

#[test]
fn an_empty_pattern_or_patterns_is_refused_as_empty() {
    let e = refused("version: 1\nrules:\n  - expected: X\n    pattern: \"\"\n");
    assert!(e.contains("pattern が空"), "{e}");
    let e = refused("version: 1\nrules:\n  - expected: X\n    patterns: []\n");
    assert!(e.contains("patterns が空"), "{e}");
}

#[test]
fn unknown_keys_and_rules_without_pattern_are_refused() {
    let e = refused(
        "version: 1\nrules:\n  - expected: X\n    pattern: a\n    options: {wordBoundary: true}\n",
    );
    assert!(e.contains("options"), "{e}");
    let e = refused("version: 1\nrules:\n  - expected: X\n");
    assert!(e.contains("pattern"), "{e}");
    let e = refused("version: 1\nimports: [other.yml]\nrules: []\n");
    assert!(e.contains("imports"), "{e}");
    let e = refused("version: 2\nrules: []\n");
    assert!(e.contains("version"), "{e}");
    let e = refused("rules:\n  - just a string\n");
    assert!(e.contains("1 番目の規則"), "{e}");
}

#[test]
fn a_dictionary_with_a_byte_order_mark_reads_as_without_it() {
    // js-yaml は先頭の BOM を外す。Windows のメモ帳などは UTF-8 に BOM を付けて保存する
    let d = load("\u{feff}version: 1\nrules:\n  - expected: X\n    pattern: a\n");
    assert_eq!(apply(&d, "a"), "X");
}

#[test]
fn a_dictionary_with_several_yaml_documents_is_refused() {
    // prh の js-yaml の load は、2 つ目の文書があれば例外にする
    let e = refused(
        "version: 1\nrules:\n  - expected: X\n    pattern: a\n---\nversion: 1\nrules:\n  - expected: Y\n    pattern: b\n",
    );
    assert!(e.contains("文書"), "{e}");
}

#[test]
fn diffs_drop_overlaps_once_as_textlint_reports_them_and_apply_drops_them_twice() {
    // prh の ChangeSet は作るときに重なりを外し、textlint-rule-prh はその diffs を報告する。applyChangeSets はもう 1 度
    // 外す。外し方は前の要素を外す前の並びで比べるので、2 度目に外れるものがある
    let d = load(
        "version: 1\nrules:\n  - expected: X\n    pattern: \"0123456789\"\n  - expected: Y\n    pattern: \"56\"\n  - expected: Z\n    pattern: \"89AB\"\n",
    );
    assert_eq!(
        d.diffs("0123456789AB").unwrap(),
        vec![
            Diff {
                start: 0,
                end: 10,
                replacement: "X".into()
            },
            Diff {
                start: 8,
                end: 12,
                replacement: "Z".into()
            },
        ]
    );
    assert_eq!(apply(&d, "0123456789AB"), "XAB");
}

#[test]
fn an_empty_patterns_beside_a_pattern_is_refused() {
    // prh の Rule は pattern と patterns の両方の空を確かめてから、pattern を使う
    let e = refused("version: 1\nrules:\n  - expected: X\n    pattern: a\n    patterns: ['']\n");
    assert!(e.contains("patterns が空"), "{e}");
    let e = refused("version: 1\nrules:\n  - expected: X\n    pattern: a\n    patterns: ''\n");
    assert!(e.contains("patterns が空"), "{e}");
}

#[test]
fn a_repeated_flag_is_refused() {
    // prh は new RegExp(source, flags) で読み、同じフラグが 2 度あれば SyntaxError になる
    let e = refused_rule("/a/gg");
    assert!(e.contains("フラグ"), "{e}");
}
