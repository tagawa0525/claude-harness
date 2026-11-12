//! textlint が prh を当てる Str のノードに当たる、原文の範囲（ADR-0085 の検査の mode）。

use std::ops::Range;

/// Markdown の Str のノードの原文の範囲（文の UTF-8 のバイト）
pub fn markdown(_text: &str) -> Vec<Range<usize>> {
    Vec::new()
}

/// コードとテキストのファイルの Str のノード（@textlint/text の行）の原文の範囲
pub fn text(_text: &str) -> Vec<Range<usize>> {
    Vec::new()
}
