//! pulldown-cmark の Text から作った Str の範囲を、micromark 2.11.4 と remark の拡張の読み方に揃える（strs.rs）。
//! どれも、pulldown-cmark が読まないか違う形に読むインラインで、Str を分けるか縮める。

use std::ops::Range;

use crate::autolink;

/// pulldown-cmark の Text と SoftBreak の続き（Str の候補）
pub struct Run {
    pub range: Range<usize>,
    /// 表のセルの中なら、そのセルの終わり
    pub cell_end: Option<usize>,
    /// インラインを持つブロックの番号
    pub block: usize,
    /// 参照のリンクの文字列の中か
    pub in_label: bool,
}

/// 補正の途中の Str
struct Piece {
    range: Range<usize>,
    block: usize,
}

pub fn refine(text: &str, runs: Vec<Run>) -> Vec<Range<usize>> {
    let mut pieces = Vec::new();
    // literal autolink の終わり。パスは空白か行の終わりまで続き、後のコードスパンや Str を含みうる
    let mut covered = 0;
    // ブロックの中の、閉じていない [ の数。micromark は閉じていないリンクの文字列の中で literal autolink を作らない
    // （syntax.js の previous(events)。閉じる前の [ の後の URL は、後で閉じてもリンクにならない）
    let mut open_brackets = (usize::MAX, 0usize);
    for mut run in runs {
        if open_brackets.0 != run.block {
            open_brackets = (run.block, 0);
        }
        // Str の先頭の逃がした文字（\`、表のセルの \| など）は、pulldown-cmark が \ を範囲から外す（mdast の text は含む）
        if text[..run.range.start].ends_with('\\')
            && text[run.range.start..].starts_with(|c: char| c.is_ascii_punctuation())
        {
            run.range.start -= 1;
        }
        for mut range in inline_notes(text, run.range) {
            if range.end <= covered {
                continue;
            }
            range.start = range.start.max(covered);
            let pieces_of_range = if run.in_label {
                vec![range.clone()]
            } else {
                autolinks(
                    text,
                    range.clone(),
                    run.cell_end,
                    &mut covered,
                    open_brackets.1,
                )
            };
            open_brackets.1 = brackets_after(&text[range], open_brackets.1);
            for piece in pieces_of_range {
                pieces.push(Piece {
                    range: piece,
                    block: run.block,
                });
            }
        }
    }
    strikethrough(text, pieces)
}

/// remark-footnotes のインラインの注（`^[…]`）で Str を分ける。注の中身は別の Str になる。pulldown-cmark は
/// インラインの注を読まない
fn inline_notes(text: &str, run: Range<usize>) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut last = run.start;
    let mut search = run.start;
    while let Some(found) = text[search..run.end].find("^[") {
        let open = search + found;
        // 逃がした \^[ は注でない（\ の数が奇数なら ^ を逃がしている）
        if text[..open]
            .bytes()
            .rev()
            .take_while(|&b| b == b'\\')
            .count()
            % 2
            == 1
        {
            search = open + 2;
            continue;
        }
        let Some(close) = closing_bracket(&text[open + 2..run.end]).map(|i| open + 2 + i) else {
            break;
        };
        if open > last {
            result.push(last..open);
        }
        if close > open + 2 {
            result.push(open + 2..close);
        }
        last = close + 1;
        search = last;
    }
    if last < run.end {
        result.push(last..run.end);
    }
    result
}

/// 角括弧の釣り合う `]` の位置（\ で逃がした括弧は数えない）
fn closing_bracket(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' if depth == 0 => return Some(i),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// s を読んだ後の、閉じていない [ の数（逃がした括弧は数えない）
fn brackets_after(s: &str, mut open: usize) -> usize {
    let mut escaped = false;
    for c in s.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' => open += 1,
            ']' => open = open.saturating_sub(1),
            _ => {}
        }
    }
    open
}

/// literal autolink（Link になり、textlint-rule-prh が見ない）を Str から外す。open は range の前の、閉じていない
/// [ の数
fn autolinks(
    text: &str,
    range: Range<usize>,
    cell_end: Option<usize>,
    covered: &mut usize,
    open: usize,
) -> Vec<Range<usize>> {
    let line_end = text[range.end..]
        .find('\n')
        .map_or(text.len(), |i| range.end + i);
    let limit = cell_end.map_or(line_end, |end| end.min(line_end));
    let mut result = Vec::new();
    let mut last = range.start;
    for link in autolink::find(text, range.start, range.end, limit) {
        if brackets_after(&text[range.start..link.start], open) > 0 {
            continue;
        }
        // 行の始めの literal autolink の前の字下げは、mdast の text に入らない
        let before = &text[last..link.start];
        let end = match before.rfind('\n') {
            Some(i) if before[i + 1..].bytes().all(|b| b == b' ' || b == b'\t') => last + i + 1,
            _ => link.start,
        };
        if end > last {
            result.push(last..end);
        }
        last = link.end;
        *covered = (*covered).max(link.end);
    }
    if last < range.end {
        result.push(last..range.end);
    }
    result
}

/// 取り消し線の印（`~` が 1 つか 2 つの並び）
struct Sequence {
    range: Range<usize>,
    block: usize,
    open: bool,
    close: bool,
}

/// micromark-extension-gfm-strikethrough 0.6.5（remark-gfm 1.0.0 は 1 つの ~ も許す）の取り消し線の印を Str から
/// 外す。取り消し線の中身は Str のまま（textlint-rule-prh は Delete の下を見る）なので、組になった印の所で Str を
/// 分ける。組にならない印は Str に残る
fn strikethrough(text: &str, pieces: Vec<Piece>) -> Vec<Range<usize>> {
    let mut sequences = Vec::new();
    for piece in &pieces {
        let mut chars = text[piece.range.clone()].char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    chars.next();
                }
                '~' => {
                    let start = piece.range.start + i;
                    let mut end = start + 1;
                    while chars.next_if(|&(_, c)| c == '~').is_some() {
                        end += 1;
                    }
                    // 3 つ以上の並びは印にならない（前の ~ が \~ なら並びに数えない）
                    let tilde_before =
                        text[..start].ends_with('~') && !text[..start].ends_with("\\~");
                    if end - start > 2 || tilde_before || text[end..].starts_with('~') {
                        continue;
                    }
                    let before = classify(text[..start].chars().next_back());
                    let after = classify(text[end..].chars().next());
                    sequences.push(Sequence {
                        range: start..end,
                        block: piece.block,
                        open: after == Class::Other
                            || (after == Class::Punctuation && before != Class::Other),
                        close: before == Class::Other
                            || (before == Class::Punctuation && after != Class::Other),
                    });
                }
                _ => {}
            }
        }
    }
    // 閉じの印ごとに、前の同じ長さの開きの印を探して組にする。組の間で組にならなかった印は、もう使わない
    let mut used = vec![false; sequences.len()];
    let mut paired = Vec::new();
    for close in 0..sequences.len() {
        if used[close] || !sequences[close].close {
            continue;
        }
        let found = (0..close).rev().find(|&open| {
            !used[open]
                && sequences[open].open
                && sequences[open].block == sequences[close].block
                && sequences[open].range.len() == sequences[close].range.len()
        });
        if let Some(open) = found {
            used[open..=close].fill(true);
            paired.push(sequences[open].range.clone());
            paired.push(sequences[close].range.clone());
        }
    }
    paired.sort_by_key(|r| r.start);
    let mut result = Vec::new();
    let mut marks = paired.iter().peekable();
    for piece in pieces {
        let mut last = piece.range.start;
        while let Some(mark) = marks.next_if(|m| m.start < piece.range.end) {
            if mark.start > last {
                result.push(last..mark.start);
            }
            last = mark.end;
        }
        if last < piece.range.end {
            result.push(last..piece.range.end);
        }
    }
    result
}

#[derive(PartialEq, Eq)]
enum Class {
    Whitespace,
    Punctuation,
    Other,
}

/// micromark の classifyCharacter（文の始めと終わりは空白）
fn classify(c: Option<char>) -> Class {
    match c {
        None => Class::Whitespace,
        Some(c) if autolink::unicode_whitespace(c) => Class::Whitespace,
        Some(c) if autolink::unicode_punctuation(c) => Class::Punctuation,
        Some(_) => Class::Other,
    }
}
