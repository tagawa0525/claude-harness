//! ファイルの全体の検査の報告。textlint 15.7.1 が textlint-rule-prh 6.0.0 の報告を並べるとおりにする（ADR-0085）。
//!
//! - 位置は BOM を除いた文の行と列。行は `\r\n`、`\r`、`\n`、U+2028、U+2029 で分け（structured-source 4.0.0）、
//!   列は行の頭からの UTF-16 の数に 1 を足す（@textlint/kernel の source-location.js）
//! - 行と列の順に、同じ位置なら報告した順に並べる（sort-messages-process.js。JavaScript の sort は安定）
//!
//! textlint は同じ位置と同じ文の報告を 1 つにする（filter-duplicated-process.js）が、prh だけの報告では起きない
//! （Str の範囲は重ならず、1 つの Str の中の一致は重なりを外した後に同じ開始を持たない）ので、まとめない。

use crate::{Dictionary, LINE_TERMINATORS, TextError, strs};

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
        Syntax::Markdown => strs::markdown(text)?,
        Syntax::Text => strs::text(text),
    };
    let bom = strs::after_bom(text);
    let lines = Lines::new(&text[bom..]);
    let mut reports = Vec::new();
    for range in ranges {
        for finding in dictionary.check(&text[range.clone()])? {
            let (line, column) = lines.position(range.start + finding.start - bom);
            reports.push(Report {
                line,
                column,
                message: format!("{} => {}", finding.actual, finding.expected),
            });
        }
    }
    reports.sort_by_key(|r| (r.line, r.column));
    Ok(reports)
}

/// 行の頭の位置
struct Lines<'a> {
    text: &'a str,
    /// 行の頭（UTF-8 のバイト）
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        let mut chars = text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            if c == '\r' && chars.next_if(|&(_, next)| next == '\n').is_some() {
                starts.push(at + 2);
            } else if LINE_TERMINATORS.contains(&c) {
                starts.push(at + c.len_utf8());
            }
        }
        Lines { text, starts }
    }

    /// at（UTF-8 のバイト）の、1 から数える行と列
    fn position(&self, at: usize) -> (usize, usize) {
        let line = self.starts.partition_point(|&start| start <= at) - 1;
        let column = self.text[self.starts[line]..at].encode_utf16().count();
        (line + 1, column + 1)
    }
}
