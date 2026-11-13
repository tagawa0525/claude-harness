//! Markdown とテキストのファイルから切り出す Str の範囲が、textlint 15.7.1 の Str のノードと同じこと（ADR-0085）。
//!
//! 期待値（tests/textlint/<入力>.strs）は tests/textlint/update.sh が textlint で作る。1 行が 1 つの Str で、
//! BOM を除いた文の中の UTF-16 の開始の位置と、原文（\、改行、復帰、tab を逃がしたもの）をタブで区切る。

use std::path::Path;

use ja_terms::strs;

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// textlint と同じ形（BOM を除いた文の UTF-16 の位置と、逃がした原文）にする
fn render(text: &str, ranges: &[std::ops::Range<usize>]) -> String {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    ranges
        .iter()
        .map(|r| {
            let index: usize = text[bom..r.start].encode_utf16().count();
            format!("{index}\t{}\n", escape(&text[r.clone()]))
        })
        .collect()
}

fn check(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/textlint");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    let expected = std::fs::read_to_string(dir.join(format!("{name}.strs"))).unwrap();
    let ranges = if name.ends_with(".markdown") {
        strs::markdown(&text)
    } else {
        strs::text(&text)
    };
    assert_eq!(render(&text, &ranges), expected, "{name}");
}

#[test]
fn markdown_blocks_are_cut_as_textlint_does() {
    check("blocks.markdown");
}

#[test]
fn markdown_inlines_are_cut_as_textlint_does() {
    check("inlines.markdown");
}

#[test]
fn markdown_edges_are_cut_as_textlint_does() {
    check("edges.markdown");
}

#[test]
fn a_byte_order_mark_and_crlf_are_cut_as_textlint_does() {
    check("bom-crlf.markdown");
}

#[test]
fn text_files_are_cut_into_lines_as_textlint_does() {
    check("lines.txt");
}

#[test]
fn lines_without_the_container_marks_start_new_paragraphs_as_micromark_2_does() {
    check("lazy.markdown");
}

#[test]
fn literal_autolinks_are_left_out_as_links() {
    check("autolinks.markdown");
}

#[test]
fn single_and_double_tilde_strikethrough_split_strs() {
    check("strike.markdown");
}

#[test]
fn front_matter_only_at_the_start_escapes_symbols_and_references_are_cut_as_textlint_does() {
    check("misc.markdown");
    check("front-toml.markdown");
}

#[test]
fn tabs_indent_list_items_by_four_columns() {
    check("tabs.markdown");
}

#[test]
fn footnote_definitions_need_four_columns_to_continue() {
    check("footnotes.markdown");
}

#[test]
fn urls_inside_open_brackets_are_not_links_and_an_escaped_note_is_text() {
    check("brackets.markdown");
}

#[test]
fn a_long_run_without_an_at_sign_is_cut_in_linear_time() {
    // メールアドレスの判定が並びを読み直すと、20 万文字で debug のビルドが 4 分ほどかかる。直した後は 1 秒に
    // ならないので、負荷のある builder でも揺れないよう、上限は 2 乗の時間との差の中で十分に緩くする
    let text = "1".repeat(200_000);
    let start = std::time::Instant::now();
    assert_eq!(strs::markdown(&text), vec![0..200_000]);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(30),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_url_right_after_a_byte_order_mark_is_a_link() {
    check("bom-www.markdown");
}

#[test]
fn an_escaped_backslash_before_a_note_and_symbol_labels_are_read_as_micromark_does() {
    check("escapes.markdown");
}
