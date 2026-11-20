//! リストの項の中の字下げしたコードブロックを、項の中身の桁（CommonMark の項の印の幅と、印の後ろの空白）から 4 桁以上の
//! 字下げで見分け、表の形の行を揃え直さないこと（#347）。項の中身の桁から 0〜3 桁の字下げの表は揃える。

use fix_markdown_lint::fix_markdown;

const TABLE: [&str; 3] = ["| a | b |", "| - | - |", "| xxx | y |"];
const ALIGNED: [&str; 3] = ["| a   | b |", "| --- | - |", "| xxx | y |"];

/// head の後ろに、indent 桁の字下げで rows の 3 行を置く
fn doc(head: &str, indent: usize, rows: [&str; 3]) -> String {
    let pad = " ".repeat(indent);
    let body: String = rows.iter().map(|r| format!("{pad}{r}\n")).collect();
    format!("{head}{body}")
}

fn assert_left_alone(head: &str, indent: usize) {
    let src = doc(head, indent, TABLE);
    assert_eq!(fix_markdown(&src), src);
}

fn assert_realigned(head: &str, indent: usize) {
    assert_eq!(
        fix_markdown(&doc(head, indent, TABLE)),
        doc(head, indent, ALIGNED)
    );
}

#[test]
fn a_table_shaped_code_block_in_an_ordered_list_item_is_left_alone() {
    assert_left_alone("1. item\n\n", 7);
    assert_left_alone("1. item\n\n", 8);
    assert_left_alone("10. item\n\n", 8);
    assert_left_alone("1) item\n\n", 7);
}

#[test]
fn a_table_shaped_code_block_in_a_bullet_list_item_is_left_alone() {
    assert_left_alone("- item\n\n", 6);
    assert_left_alone("* item\n\n", 7);
    assert_left_alone("+ item\n\n", 6);
}

#[test]
fn a_table_shaped_code_block_in_a_nested_list_item_is_left_alone() {
    assert_left_alone("- outer\n  - inner\n\n", 8);
    assert_left_alone("1. outer\n   1. inner\n\n", 10);
    assert_left_alone("- outer\n  10. inner\n\n", 10);
}

#[test]
fn the_content_column_follows_the_spaces_after_the_marker() {
    // 印の後ろの空白が 1〜4 桁なら、中身はその後ろから始まる
    assert_left_alone("-   item\n\n", 8);
    assert_realigned("-   item\n\n", 7);
    // 5 桁以上なら、中身は印の後ろの 1 桁から始まる（項の最初の行が字下げしたコードブロック）
    assert_left_alone("-     code\n\n", 6);
}

#[test]
fn a_table_in_a_list_item_within_three_columns_of_its_content_is_realigned() {
    assert_realigned("1. item\n\n", 3);
    assert_realigned("1. item\n\n", 6);
    assert_realigned("- item\n\n", 2);
    assert_realigned("- item\n\n", 5);
    assert_realigned("- outer\n  - inner\n\n", 4);
    assert_realigned("- outer\n  - inner\n\n", 7);
}

#[test]
fn a_table_after_a_nested_item_ends_belongs_to_the_outer_item() {
    // 外の項の中身の桁（2）から 4 桁なのでコード。内の項（4）の中なら表になる桁
    assert_left_alone("- outer\n  - inner\n\n  text\n\n", 6);
    assert_realigned("- outer\n  - inner\n\n  text\n\n", 5);
}

#[test]
fn a_list_marker_inside_a_fence_does_not_open_a_list_item() {
    assert_left_alone("- item\n\n  ```text\n  - x\n  ```\n\n", 6);
}

#[test]
fn a_thematic_break_is_not_a_list_item() {
    assert_left_alone("* * *\n\n", 4);
    assert_left_alone("- - -\n\n", 4);
}

#[test]
fn a_list_item_starting_with_a_blank_line_ends_at_a_second_blank_line() {
    // 印だけの行の次が空の行なら、項は空で閉じる（CommonMark）。その後の 4 桁の字下げはリストの外のコード
    assert_left_alone("-\n\n", 4);
    // 印だけの行の次の行が中身なら、項の中
    assert_realigned("-\n  text\n\n", 4);
}

#[test]
fn a_lazy_continuation_line_keeps_the_list_item_open() {
    // 字下げの足りない段落の続きの行（遅延継続行）も項の中
    assert_realigned("- item\ncontinued\n\n", 4);
    assert_left_alone("- item\ncontinued\n\n", 6);
}

#[test]
fn a_fence_after_a_lazy_continuation_line_protects_its_content() {
    let src = "1. item\n  continued\n\n    ```text\n   | a | b |\n   | - | - |\n   | xxx | y |\n    ```\n";
    assert_eq!(fix_markdown(src), src);
    assert_eq!(
        fix_markdown("1. item\n  continued\n\n    ```\n    fn main() {}\n    ```\n"),
        "1. item\n  continued\n\n    ```rust\n    fn main() {}\n    ```\n"
    );
}

#[test]
fn a_dedented_line_inside_a_fence_closes_the_list_item() {
    assert_table_in_reopened_fence(4);
}

/// `x` が項とフェンスを閉じ、次の `  ```` は閉じのないフェンスを開く（言語の推定が言語を付ける）。表はその中身
fn assert_table_in_reopened_fence(indent: usize) {
    let src = doc("- item\n\n  ```text\n  code\nx\n  ```\n\n", indent, TABLE);
    let want = src.replacen("x\n  ```\n", "x\n  ```text\n", 1);
    assert_eq!(fix_markdown(&src), want);
}

#[test]
fn an_ordered_marker_other_than_one_does_not_interrupt_a_paragraph() {
    assert_left_alone("text\n2. not a list\n\n", 4);
    // 番号なしの印と 1 の番号は段落を区切って項を開く
    assert_realigned("text\n- item\n\n", 4);
    assert_realigned("text\n1. item\n\n", 4);
}

#[test]
fn a_digit_outside_ascii_is_not_an_ordered_list_marker() {
    assert_left_alone("\u{661}. item\n\n", 4);
    assert_left_alone("\u{ff11}. item\n\n", 4);
}

#[test]
fn a_marker_followed_only_by_a_non_ascii_space_is_not_a_list_item() {
    assert_left_alone("-\u{3000}\n  text\n\n", 4);
}

#[test]
fn a_list_item_opened_on_the_marker_line_of_another_is_counted() {
    // `- 1. item` の内の項の中身の桁は 5
    assert_realigned("- 1. item\n\n", 6);
    assert_left_alone("- 1. item\n\n", 9);
    let src =
        "- 1. item\n\n      ```text\n     | a | b |\n     | - | - |\n     | xxx | y |\n      ```\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn a_line_after_an_item_starting_with_a_non_paragraph_block_is_not_a_lazy_continuation() {
    // 印の行の中身が字下げしたコードブロックか見出しなら、段落の続きの行は無く、字下げの無い行がリストを閉じる
    assert_left_alone("-     code\nfoo\n\n", 4);
    assert_left_alone("- # h\nfoo\n\n", 4);
}

#[test]
fn a_dedented_line_closes_a_fence_in_a_list_item_for_the_table_pass() {
    assert_table_in_reopened_fence(0);
}

#[test]
fn a_fence_on_the_marker_line_protects_its_content() {
    let src = "- ```text\n  | a | b |\n  | - | - |\n  | xxx | y |\n  ```\n";
    assert_eq!(fix_markdown(src), src);
    assert_eq!(
        fix_markdown("- ```\n  fn main() {}\n  ```\n"),
        "- ```rust\n  fn main() {}\n  ```\n"
    );
}

#[test]
fn a_closing_fence_is_indented_less_than_four_columns_from_the_item_content() {
    // 5 桁の ``` は、項の外（中身の桁 0）では 4 桁以上の字下げで、フェンスの中身
    let src = "   ```text\n     ```\n| a | b |\n| - | - |\n| xxx | y |\n```\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn a_paragraph_without_indent_ends_the_list() {
    assert_left_alone("- item\n\ntext\n\n", 4);
}

#[test]
fn a_line_after_a_table_in_a_list_item_is_not_a_lazy_continuation() {
    // GFM の表は遅延継続行を持たないので、字下げの無い行は表と項を閉じる。その後の 6 桁はリストの外のコード
    assert_left_alone("  - inner\n    | x | y |\n    | - | - |\ntext\n\n", 6);
}

#[test]
fn an_html_block_ends_the_list_item() {
    // HTML のブロック（`<div>`）は段落を区切って項を閉じる。その後の 4 桁はリストの外のコード
    assert_left_alone("- item\n<div>\n\n", 4);
}

#[test]
fn a_heading_mark_after_a_non_ascii_space_is_a_lazy_continuation() {
    // 全角の空白は Markdown の字下げでないので、` 　# x` は見出しでなく段落の続きで、項を開いたままにする。
    // 項の中のフェンスの中身は書き換えない
    let src =
        "- item\n \u{3000}# x\n\n    ```text\n  | a | b |\n  | - | - |\n  | xxx | y |\n    ```\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn a_fence_in_a_nested_list_item_is_left_alone_and_gets_a_language() {
    let src = "1. outer\n   - inner\n\n     ```text\n     | a | b |\n     | - | - |\n     | xxx | y |\n     ```\n";
    assert_eq!(fix_markdown(src), src);
    assert_eq!(
        fix_markdown("1. outer\n   - inner\n\n     ```\n     fn main() {}\n     ```\n"),
        "1. outer\n   - inner\n\n     ```rust\n     fn main() {}\n     ```\n"
    );
}
