//! 表の揃え直しが、コードの中身の表の形の行を整形しないこと（#99）。

use fix_markdown_lint::fix_markdown;

#[test]
fn a_table_shaped_row_in_a_fence_indented_in_a_list_item_is_left_alone() {
    let src = "10. step\n\n    ```text\n    | a | b |\n    | - | - |\n    | xxx | y |\n    ```\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn a_table_shaped_row_in_an_indented_code_block_is_left_alone() {
    let src = "text\n\n    | a | b |\n    | - | - |\n    | xxx | y |\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn an_indented_table_in_a_list_item_is_realigned_keeping_its_indent() {
    assert_eq!(
        fix_markdown("- item\n\n    | a | b |\n    | - | - |\n    | xxx | y |\n"),
        "- item\n\n    | a   | b |\n    | --- | - |\n    | xxx | y |\n"
    );
}

#[test]
fn a_table_in_a_blockquote_is_left_alone() {
    let src = "> | a | b |\n> | - | - |\n> | xxx | y |\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn backticks_in_a_top_level_indented_code_block_are_not_a_fence() {
    let src = "text\n\n    ```\n    let x = 1;\n    ```\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn an_indented_fence_line_inside_a_fence_does_not_close_it() {
    let src = "```text\n    ```\n| a | b |\n| - | - |\n| xxx | y |\n```\n";
    assert_eq!(fix_markdown(src), src);
}

#[test]
fn a_fence_in_a_list_item_without_a_language_gets_one_and_closes_at_its_indent() {
    assert_eq!(
        fix_markdown("- item\n\n    ```\n    let x = 1;\n    ```\n"),
        "- item\n\n    ```rust\n    let x = 1;\n    ```\n"
    );
}
