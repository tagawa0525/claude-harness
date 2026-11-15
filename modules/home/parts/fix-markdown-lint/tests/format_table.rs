//! `format_table` の揃え方（markdownlint の MD056、MD060 と食い違わないこと）。

use fix_markdown_lint::format_table;

fn fmt(rows: &[&str]) -> Vec<String> {
    format_table(rows)
}

#[test]
fn an_escaped_pipe_stays_in_its_cell() {
    assert_eq!(
        fmt(&["| a | b |", "| - | - |", "| x \\| y | z |"]),
        ["| a      | b |", "| ------ | - |", "| x \\| y | z |"]
    );
}

#[test]
fn an_empty_column_and_a_narrow_aligned_column_keep_the_separator_as_wide_as_the_cells() {
    assert_eq!(
        fmt(&["| | x |", "| --- | --- |", "| | y |"]),
        ["|   | x |", "| - | - |", "|   | y |"]
    );
    assert_eq!(
        fmt(&["| a | bb |", "| :-: | --- |", "| c | d |"]),
        ["|  a  | bb |", "| :-: | -- |", "|  c  | d  |"]
    );
    assert_eq!(
        fmt(&["| a | b |", "| -: | --- |", "| c | d |"]),
        ["|  a | b |", "| -: | - |", "|  c | d |"]
    );
}

#[test]
fn a_row_with_more_cells_than_the_header_leaves_the_table_as_written() {
    let table = ["| a | b |", "| - | - |", "| 1 | 2 | 3 |"];
    assert_eq!(fmt(&table), table);
}

#[test]
fn an_explicit_left_alignment_keeps_its_colon() {
    assert_eq!(
        fmt(&["| a | b |", "| :-- | --- |", "| ccc | d |"]),
        ["| a   | b |", "| :-- | - |", "| ccc | d |"]
    );
}

#[test]
fn keeps_an_escaped_pipe_next_to_the_closing_pipe() {
    assert_eq!(
        fmt(&["| a | b |", "| - | - |", "| x | \\||"]),
        ["| a | b  |", "| - | -- |", "| x | \\| |"]
    );
}

#[test]
#[ignore = "RED: #221 cells() drops a closing-less trailing escaped pipe"]
fn a_row_without_a_closing_pipe_keeps_a_trailing_escaped_pipe() {
    assert_eq!(
        fmt(&["| a | b |", "| - | - |", "| x | y \\|"]),
        ["| a | b    |", "| - | ---- |", "| x | y \\| |"]
    );
}
