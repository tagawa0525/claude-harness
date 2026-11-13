//! prh の段落の `prh:disable` の指示（prh 5.4.4 の lib/paragraph.js）。
//!
//! 指示は `/^(?:.*?)prh:disable(?::([^\n\s]*))?/gm` で探す。行（JavaScript の行の終わりで分けたもの）ごとに最初の
//! `prh:disable` だけを読み、`:` の後が無いか空なら段落のすべての一致を、あればその pattern に一致する一致と、
//! 行の頭から指示の終わりまでの範囲と重なる一致を外す。

use regress::Regex;

use crate::{Diff, LINE_TERMINATORS, TextError};

/// 1 つの段落の指示
pub(crate) struct Pragmas {
    ignore_all: bool,
    /// 行の頭から指示の終わりまで（段落の中の UTF-8 のバイト）
    ranges: Vec<(usize, usize)>,
    patterns: Vec<Regex>,
}

/// JavaScript の `\s`（WhiteSpace と LineTerminator）
fn js_space(c: char) -> bool {
    const SPACES: [char; 14] = [
        '\t', '\n', '\u{b}', '\u{c}', '\r', ' ', '\u{a0}', '\u{1680}', '\u{2028}', '\u{2029}',
        '\u{202f}', '\u{205f}', '\u{3000}', '\u{feff}',
    ];
    SPACES.contains(&c) || ('\u{2000}'..='\u{200a}').contains(&c)
}

impl Pragmas {
    pub(crate) fn read(paragraph: &str) -> Result<Self, TextError> {
        const PRAGMA: &str = "prh:disable";
        let mut pragmas = Pragmas {
            ignore_all: false,
            ranges: Vec::new(),
            patterns: Vec::new(),
        };
        let mut line_start = 0;
        for line in paragraph.split(LINE_TERMINATORS) {
            let start = line_start;
            // 区切りの行の終わりは 1 文字（\r\n も \r と \n の 2 つの行の終わり）
            line_start += line.len() + line_start_gap(paragraph, start + line.len());
            let Some(at) = line.find(PRAGMA) else {
                continue;
            };
            let rest = &line[at + PRAGMA.len()..];
            let Some(after) = rest.strip_prefix(':') else {
                pragmas.ignore_all = true;
                continue;
            };
            let length = after
                .find(|c| c == '\n' || js_space(c))
                .unwrap_or(after.len());
            let pattern = &after[..length];
            if pattern.is_empty() {
                pragmas.ignore_all = true;
                continue;
            }
            let tail = start + at + PRAGMA.len() + 1 + length;
            pragmas.ranges.push((start, tail));
            // prh は new RegExp("(" + pattern + ")", "g") で読む（u のフラグは無い）
            let regex = Regex::with_flags(&format!("({pattern})"), "").map_err(|e| {
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
                // u のフラグが無いので、V8 と同じく UTF-16 の単位で照合する
                let matched: Vec<u16> = text[d.start..d.end].encode_utf16().collect();
                self.patterns
                    .iter()
                    .all(|p| p.find_from_ucs2(&matched, 0).next().is_none())
            })
            .collect()
    }
}

/// paragraph の at にある行の終わりの長さ（UTF-8 のバイト。文の終わりなら 0）
fn line_start_gap(paragraph: &str, at: usize) -> usize {
    paragraph[at..].chars().next().map_or(0, char::len_utf8)
}
