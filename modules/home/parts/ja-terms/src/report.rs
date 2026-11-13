//! ファイルの全体の検査の報告。textlint 15.7.1 が textlint-rule-prh 6.0.0 の報告を並べるとおりにする（ADR-0085）。
//!
//! - 位置は BOM を除いた文の行と列。行は `\r\n`、`\r`、`\n`、U+2028、U+2029 で分け（structured-source 4.0.0）、
//!   列は行の頭からの UTF-16 の数に 1 を足す（@textlint/kernel の source-location.js）
//! - 同じ位置と同じ文の報告は 1 つにする（filter-duplicated-process.js）
//! - 行と列の順に、同じ位置なら報告した順に並べる（sort-messages-process.js。JavaScript の sort は安定）

use std::collections::HashSet;

use crate::{Dictionary, TextError, strs};

/// ファイルの読み方（textlint のプラグイン）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    /// @textlint/textlint-plugin-markdown
    Markdown,
    /// @textlint/textlint-plugin-text（コードとテキストのファイル）
    Text,
}

/// 1 件の報告
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// 1 から数える行
    pub line: usize,
    /// 1 から数える列（UTF-16）
    pub column: usize,
    /// 「誤り => 正しい語」
    pub message: String,
}

/// text の Str に辞書を当て、textlint の報告の順に並べる
pub fn check(
    dictionary: &Dictionary,
    text: &str,
    syntax: Syntax,
) -> Result<Vec<Report>, TextError> {
    let ranges = match syntax {
        Syntax::Markdown => strs::markdown(text),
        Syntax::Text => strs::text(text),
    };
    let bom = strs::after_bom(text);
    let lines = Lines::new(&text[bom..]);
    // (UTF-16 の位置、報告)
    let mut reports: Vec<(usize, Report)> = Vec::new();
    for range in ranges {
        for finding in dictionary.check(&text[range.clone()])? {
            let at = range.start + finding.start - bom;
            let (index, line, column) = lines.position(at);
            reports.push((
                index,
                Report {
                    line,
                    column,
                    message: format!("{} => {}", finding.actual, finding.expected),
                },
            ));
        }
    }
    // 同じものが後にあれば外す（範囲は位置から 1 つなので、位置と文で比べる）
    let mut seen = HashSet::new();
    let mut kept: Vec<Report> = reports
        .into_iter()
        .rev()
        .filter(|(index, report)| seen.insert((*index, report.message.clone())))
        .map(|(_, report)| report)
        .collect();
    kept.reverse();
    kept.sort_by_key(|r| (r.line, r.column));
    Ok(kept)
}

/// 行の頭の位置
struct Lines<'a> {
    text: &'a str,
    /// 行の頭の (UTF-8 のバイト、UTF-16 の位置)
    starts: Vec<(usize, usize)>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = vec![(0, 0)];
        let mut unit = 0;
        let mut chars = text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            unit += c.len_utf16();
            let crlf = c == '\r' && chars.peek().is_some_and(|&(_, next)| next == '\n');
            if crlf {
                chars.next();
                unit += 1;
                starts.push((at + 2, unit));
            } else if matches!(c, '\r' | '\n' | '\u{2028}' | '\u{2029}') {
                starts.push((at + c.len_utf8(), unit));
            }
        }
        Lines { text, starts }
    }

    /// at（UTF-8 のバイト）の UTF-16 の位置と、1 から数える行と列
    fn position(&self, at: usize) -> (usize, usize, usize) {
        let line = self.starts.partition_point(|&(byte, _)| byte <= at) - 1;
        let (byte, unit) = self.starts[line];
        let column = self.text[byte..at].encode_utf16().count();
        (unit + column, line + 1, column + 1)
    }
}
