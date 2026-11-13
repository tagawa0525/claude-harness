//! textlint が prh を当てる Str のノードに当たる、原文の範囲（ADR-0085 の検査の mode）。
//!
//! textlint 15.7.1 の Markdown は @textlint/markdown-to-ast（remark-parse 9 の micromark 2.11.4、remark-gfm 1.0.0、
//! remark-frontmatter 3.0.0、remark-footnotes 3.0.0）が読み、mdast の text のノードを Str にする。textlint-rule-prh 6.0.0
//! は既定で Link、BlockQuote、Emphasis の下の Str を見ない。コードとテキストのファイルは @textlint/text が 1 行ずつ
//! Str にする。範囲は tests/textlint の期待値（textlint 自身の答え）と比べて確かめる。
//!
//! ブロックの構造と大半のインラインは pulldown-cmark で読み、micromark 2.11.4 と読み方の違うところを、読む前の
//! 置き換え（front matter、記号）と読み直し（lazy continuation）と、読んだ後の補正（inline.rs）で揃える。

use std::borrow::Cow;
use std::ops::Range;
use std::sync::OnceLock;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use regress::Regex;

use crate::inline::{self, Run};

/// textlint は先頭の BOM を外して読む
fn after_bom(text: &str) -> usize {
    if text.starts_with('\u{feff}') { 3 } else { 0 }
}

/// Markdown の Str のノードの原文の範囲（文の UTF-8 のバイト）
pub fn markdown(text: &str) -> Vec<Range<usize>> {
    let start = after_bom(text);
    let body = &text[start..];
    let blanked = without_front_matter(body);
    let neutral = without_symbols(&blanked);
    let original = neutral.as_ref();
    // micromark 2.11.4 は lazy continuation（リストの項目の字下げか引用の > の足りない、段落の続きの行）を読まず、
    // その行で項目と引用を閉じて新しい段落にする。pulldown-cmark は CommonMark のとおり前の段落の続きと読むので、
    // 最初の lazy な行の前に空行を入れて読み直す。後の行が lazy かは、読み直した構造で決まる（前の行で項目が
    // 閉じると、後の行は別の項目の続きになりうる）ので、1 度に 1 行ずつ入れる
    let mut breaks: Vec<usize> = Vec::new();
    let runs = loop {
        let source = with_breaks(original, &breaks);
        match parse(&source) {
            Parsed::Lazy(line) => {
                breaks.push(to_original(line, &breaks));
                breaks.sort_unstable();
            }
            Parsed::Runs(runs) => {
                let back = |p: usize| start + to_original(p, &breaks);
                break runs
                    .into_iter()
                    .map(|r| Run {
                        range: back(r.range.start)..back(r.range.end),
                        cell_end: r.cell_end.map(back),
                        block: r.block,
                    })
                    .collect::<Vec<_>>();
            }
        }
    };
    inline::refine(text, runs)
}

/// remark-frontmatter の front matter（文の先頭の、開きと閉じの柵で囲んだ行）を、改行を残して空白にする。
/// pulldown-cmark の metadata block は文の途中の `---`…`...` も読むので使わない
fn without_front_matter(text: &str) -> Cow<'_, str> {
    let Some(end) = front_matter_end(text) else {
        return Cow::Borrowed(text);
    };
    let mut out: Vec<u8> = text.as_bytes().to_vec();
    for b in &mut out[..end] {
        if *b != b'\n' && *b != b'\r' {
            *b = b' ';
        }
    }
    Cow::Owned(String::from_utf8(out).expect("ASCII の空白に置き換えた"))
}

/// micromark-extension-frontmatter 0.2.2 の柵（textlint の markdown-to-ast が渡す yaml、toml と 3 つの json。最初の
/// 文字が同じなら、この順に試す）。柵の行は柵の後に空白と tab だけを許し、閉じの柵が無ければ front matter でない
const FENCES: [(&str, &str); 5] = [
    ("---", "---"),
    ("---json", "---"),
    ("+++", "+++"),
    (";;;", ";;;"),
    ("{", "}"),
];

/// front matter の終わり（閉じの柵の行の、改行の前）
fn front_matter_end(text: &str) -> Option<usize> {
    let fence = |line: &str, sequence: &str| {
        line.strip_suffix('\r')
            .unwrap_or(line)
            .strip_prefix(sequence)
            .is_some_and(|rest| rest.bytes().all(|b| b == b' ' || b == b'\t'))
    };
    let first_end = text.find('\n')?;
    let (_, close) = FENCES
        .iter()
        .find(|(open, _)| fence(&text[..first_end], open))?;
    let mut line_start = first_end + 1;
    while line_start <= text.len() {
        let line_end = text[line_start..]
            .find('\n')
            .map_or(text.len(), |i| line_start + i);
        let line = &text[line_start..line_end];
        if fence(line, close) {
            return Some(line_start + line.strip_suffix('\r').unwrap_or(line).len());
        }
        line_start = line_end + 1;
    }
    None
}

/// ASCII でない記号（Unicode の S のカテゴリ）を、同じバイト数の文字に置き換える。CommonMark 0.31 の
/// pulldown-cmark は強調の区切り（`*`、`_`）の前後の記号を句読点に数え、micromark 2.11.4 は数えない（`C*×…*` を
/// micromark は強調と読む）。記号は区切りの判定にしか効かず、Str は元の文から切り出すので、置き換えても範囲は
/// 変わらない
fn without_symbols(text: &str) -> Cow<'_, str> {
    static SYMBOL: OnceLock<Regex> = OnceLock::new();
    let symbol = SYMBOL
        .get_or_init(|| Regex::with_flags(r"(?=[^\x00-\x7f])\p{S}", "u").expect("固定の正規表現"));
    let mut found = symbol.find_iter(text).peekable();
    if found.peek().is_none() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in found {
        out.push_str(&text[last..m.start()]);
        out.push(match m.end() - m.start() {
            2 => 'é',
            3 => 'あ',
            _ => '𠀋',
        });
        last = m.end();
    }
    out.push_str(&text[last..]);
    Cow::Owned(out)
}

/// 元の文の breaks の位置（どれも行の始め）の前に空行を入れる
fn with_breaks(original: &str, breaks: &[usize]) -> String {
    let mut source = String::with_capacity(original.len() + breaks.len());
    let mut last = 0;
    for &b in breaks {
        source.push_str(&original[last..b]);
        source.push('\n');
        last = b;
    }
    source.push_str(&original[last..]);
    source
}

/// 空行を入れた文の位置を、元の文の位置にする（k 番目に入れた行は、入れた文の breaks[k] + k にある）
fn to_original(p: usize, breaks: &[usize]) -> usize {
    p - breaks
        .iter()
        .enumerate()
        .take_while(|&(k, &b)| b + k < p)
        .count()
}

/// 開いているコンテナ（lazy continuation の判定に使う）
enum Container {
    Quote,
    /// 項目と脚注の定義の中身の桁（行の始めから）
    Item(usize),
}

enum Parsed {
    /// 最初の lazy continuation の行の始め
    Lazy(usize),
    Runs(Vec<Run>),
}

fn parse(source: &str) -> Parsed {
    // 取り消し線は inline.rs が micromark の規則で読む（pulldown-cmark は 1 つの ~ を読まない）
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_FOOTNOTES;
    let mut runs = Vec::new();
    let mut containers = Vec::new();
    // 見ないノード（textlint-rule-prh が子を見ないノードと、Str を持たないブロック）の深さ
    let mut ignored = 0usize;
    // 開いているリンクごとに、見ないリンクか（参照のリンクは mdast の linkReference で、textlint-rule-prh は見る）
    let mut links = Vec::new();
    // 開いている表のセルの終わり
    let mut cell_end = None;
    // インラインを持つブロックの番号（取り消し線の組はブロックの中で作る）
    let mut block = 0;
    // 続いている Str（mdast の text は、行の終わりをはさんだ文字の並びを 1 つのノードにする）
    let mut current: Option<Range<usize>> = None;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        if let Event::SoftBreak = event {
            let line = range.start + source[range.start..].find('\n').map_or(0, |i| i + 1);
            if !continues(source, line, &containers) {
                return Parsed::Lazy(line);
            }
        }
        match event {
            Event::Text(_) | Event::SoftBreak if ignored == 0 => {
                current = Some(match current {
                    Some(c) => c.start..range.end,
                    None => range,
                });
                continue;
            }
            _ => runs.extend(current.take().map(|range| Run {
                range,
                cell_end,
                block,
            })),
        }
        if let Event::Start(tag) = &event
            && !inline_tag(tag)
        {
            block += 1;
        }
        if let Event::End(tag) = &event
            && !inline_tag_end(tag)
        {
            block += 1;
        }
        match event {
            Event::Start(Tag::Link { link_type, .. }) => {
                let ignore = matches!(
                    link_type,
                    LinkType::Inline | LinkType::Autolink | LinkType::Email
                );
                ignored += usize::from(ignore);
                links.push(ignore);
            }
            Event::End(TagEnd::Link) => {
                ignored -= usize::from(links.pop().expect("開いたリンク"));
            }
            Event::Start(Tag::TableCell) => cell_end = Some(range.end),
            Event::End(TagEnd::TableCell) => cell_end = None,
            Event::Start(Tag::Item) => {
                containers.push(Container::Item(item_indent(source, range.start)))
            }
            Event::End(TagEnd::Item) => {
                containers.pop();
            }
            // remark-footnotes の脚注の定義は、続きの行に 4 桁の字下げを求める
            Event::Start(Tag::FootnoteDefinition(_)) => {
                let line_start = source[..range.start].rfind('\n').map_or(0, |i| i + 1);
                let indent = column(&source.as_bytes()[line_start..range.start]) + 4;
                containers.push(Container::Item(indent));
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                containers.pop();
            }
            Event::Start(tag) if ignores(&tag) => {
                if matches!(tag, Tag::BlockQuote(_)) {
                    containers.push(Container::Quote);
                }
                ignored += 1;
            }
            Event::End(tag) if ignores_end(&tag) => {
                if matches!(tag, TagEnd::BlockQuote(_)) {
                    containers.pop();
                }
                ignored -= 1;
            }
            _ => {}
        }
    }
    runs.extend(current.map(|range| Run {
        range,
        cell_end,
        block,
    }));
    Parsed::Runs(runs)
}

fn inline_tag(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Link { .. } | Tag::Image { .. }
    )
}

fn inline_tag_end(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link | TagEnd::Image
    )
}

/// 行の中の桁（tab は次の 4 の倍数の桁まで進める）
fn column(line: &[u8]) -> usize {
    line.iter().fold(0, |col, &b| {
        if b == b'\t' {
            col + 4 - col % 4
        } else {
            col + 1
        }
    })
}

/// 項目の印（start の位置）の後の中身の桁。印の後の空白が 1〜4 桁ならその後、5 桁以上か行の終わりなら印と空白
/// 1 つの後
fn item_indent(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    // pulldown-cmark の Item の範囲は、印の前の字下げか、前の行の改行から始まりうる
    let start = start
        + bytes[start..]
            .iter()
            .take_while(|&&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .count();
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let marker = match bytes.get(start) {
        Some(b'-' | b'*' | b'+') => 1,
        _ => {
            bytes[start..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count()
                + 1
        }
    };
    let after = (start + marker).min(bytes.len());
    let spaces = bytes[after..]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count();
    let at_end = matches!(bytes.get(after + spaces), None | Some(b'\n' | b'\r'));
    let marker_end = column(&bytes[line_start..after]);
    let content = column(&bytes[line_start..after + spaces]);
    if at_end || content - marker_end >= 5 {
        marker_end + 1
    } else {
        content
    }
}

/// line で始まる行が、開いているコンテナ（外から順）の続きの印（引用の >、項目と脚注の定義の字下げ）を持つか
fn continues(source: &str, line: usize, containers: &[Container]) -> bool {
    let bytes = source.as_bytes();
    let mut pos = line;
    for container in containers {
        match *container {
            Container::Quote => {
                let mut p = pos;
                while p < pos + 3 && bytes.get(p) == Some(&b' ') {
                    p += 1;
                }
                if bytes.get(p) != Some(&b'>') {
                    return false;
                }
                p += 1;
                if matches!(bytes.get(p), Some(b' ' | b'\t')) {
                    p += 1;
                }
                pos = p;
            }
            Container::Item(indent) => {
                while column(&bytes[line..pos]) < indent {
                    match bytes.get(pos) {
                        Some(b' ' | b'\t') => pos += 1,
                        _ => return false,
                    }
                }
            }
        }
    }
    true
}

fn ignores(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Image { .. } | Tag::BlockQuote(_) | Tag::Emphasis | Tag::CodeBlock(_) | Tag::HtmlBlock
    )
}

fn ignores_end(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Image
            | TagEnd::BlockQuote(_)
            | TagEnd::Emphasis
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
    )
}

/// コードとテキストのファイルの Str のノード（@textlint/text の行）の原文の範囲。`\r?\n` で行に分け、空の行は
/// Str にしない
pub fn text(text: &str) -> Vec<Range<usize>> {
    let start = after_bom(text);
    let mut result = Vec::new();
    let mut line_start = start;
    let ends = text[start..]
        .match_indices('\n')
        .map(|(i, _)| (start + i, true))
        .chain(std::iter::once((text.len(), false)));
    for (end, newline) in ends {
        let line = &text[line_start..end];
        let line = match line.strip_suffix('\r') {
            Some(l) if newline => l,
            _ => line,
        };
        if !line.is_empty() {
            result.push(line_start..line_start + line.len());
        }
        line_start = end + 1;
    }
    result
}
