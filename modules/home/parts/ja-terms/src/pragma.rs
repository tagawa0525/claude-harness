//! prh の段落の `prh:disable` の指示（prh 5.4.4 の lib/paragraph.js）。
//!
//! 指示は `/^(?:.*?)prh:disable(?::([^\n\s]*))?/gm` で探す。行（JavaScript の行の終わりで分けたもの）ごとに最初の
//! `prh:disable` だけを読み、`:` の後が無いか空なら段落のすべての一致を、あればその pattern に一致する一致と、
//! 行の頭から指示の終わりまでの範囲と重なる一致を外す。

use regress::Regex;

use crate::autolink::unicode_whitespace;
use crate::{Diff, LINE_TERMINATORS, TextError};

/// 1 つの段落の指示
pub(crate) struct Pragmas {
    ignore_all: bool,
    /// 行の頭から指示の終わりまで（段落の中の UTF-8 のバイト）
    ranges: Vec<(usize, usize)>,
    patterns: Vec<Regex>,
}

impl Pragmas {
    pub(crate) fn read(paragraph: &str) -> Result<Self, TextError> {
        const PRAGMA: &str = "prh:disable";
        let mut pragmas = Pragmas {
            ignore_all: false,
            ranges: Vec::new(),
            patterns: Vec::new(),
        };
        // ほとんどの段落に指示は無い
        if !paragraph.contains(PRAGMA) {
            return Ok(pragmas);
        }
        let mut next = 0;
        // 行の終わりは 1 文字ずつ（\r\n も \r と \n の 2 つの行の終わり）
        for with_end in paragraph.split_inclusive(LINE_TERMINATORS) {
            let start = next;
            next += with_end.len();
            let line = with_end.trim_end_matches(LINE_TERMINATORS);
            let Some(at) = line.find(PRAGMA) else {
                continue;
            };
            let rest = &line[at + PRAGMA.len()..];
            let Some(after) = rest.strip_prefix(':') else {
                pragmas.ignore_all = true;
                continue;
            };
            let length = after.find(unicode_whitespace).unwrap_or(after.len());
            let pattern = &after[..length];
            if pattern.is_empty() {
                pragmas.ignore_all = true;
                continue;
            }
            let tail = start + at + PRAGMA.len() + 1 + length;
            pragmas.ranges.push((start, tail));
            // prh は new RegExp("(" + pattern + ")", "g") で読む（u のフラグは無い）。V8 は pattern も UTF-16 の単位で
            // 読むので、サロゲートペアの文字は 2 単位の並びにする
            let source = format!("({pattern})");
            let regex =
                Regex::from_unicode(source.encode_utf16().map(u32::from), "").map_err(|e| {
                    TextError(format!(
                        "prh:disable:{pattern}: 正規表現として読めない: {e}"
                    ))
                })?;
            pragmas.patterns.push(regex);
        }
        Ok(pragmas)
    }

    /// 段落（文の中の位置 offset）の一致 diffs から、指示で外すものを除く。text は文の全体
    pub(crate) fn filter(&self, text: &str, offset: usize, diffs: Vec<Diff>) -> Vec<Diff> {
        if self.ignore_all {
            return Vec::new();
        }
        diffs
            .into_iter()
            .filter(|d| {
                // diff.js の isCollide。開始が同じなら重なりとみなさない
                self.ranges.iter().all(|&(start, tail)| {
                    let (start, tail) = (offset + start, offset + tail);
                    !(start < d.start && d.start < tail || d.start < start && start < d.end)
                })
            })
            .filter(|d| {
                if self.patterns.is_empty() {
                    return true;
                }
                // u のフラグが無いので、V8 と同じく UTF-16 の単位で照合する
                let matched: Vec<u16> = text[d.start..d.end].encode_utf16().collect();
                self.patterns
                    .iter()
                    .all(|p| p.find_from_ucs2(&matched, 0).next().is_none())
            })
            .collect()
    }
}
