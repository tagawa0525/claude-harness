//! GFM の URL とメールアドレスの literal autolink（`https://…`、`www.…`、`a@b.c`）を探す。textlint の Markdown
//! （remark-gfm 1.0.0）はこれを Link にし、textlint-rule-prh は Link の下を見ない。pulldown-cmark は literal autolink を
//! 読まないので、micromark-extension-gfm-autolink-literal 0.5.7 の syntax.js の判定を移す。
//!
//! syntax.js が見る「前の文字」は、この文字の直前の原文の文字にする（micromark の previous は直前に読んだ文字）。
//! 角括弧が閉じていないリンクの文字列の中では literal autolink を作らない規則（syntax.js の previous(events)）は
//! 移していない。

use std::ops::Range;
use std::sync::OnceLock;

use regress::Regex;

/// text の from..until で始まる literal autolink の範囲を、先頭から並べる。limit は行の終わりか表のセルの終わり
/// （literal autolink はそこを越えない）
pub fn find(text: &str, from: usize, until: usize, limit: usize) -> Vec<Range<usize>> {
    let chars: Vec<(usize, char)> = text[from..limit]
        .char_indices()
        .map(|(p, c)| (from + p, c))
        .collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() && chars[i].0 < until {
        let previous = match i {
            0 => text[..from].chars().next_back(),
            _ => Some(chars[i - 1].1),
        };
        // syntax.js の text の hook は、文字ごとに email、それから http か www を試す
        let end = email(&chars, i, previous)
            .or_else(|| http(&chars, i, previous))
            .or_else(|| www(&chars, i, previous));
        match end {
            Some(end) => {
                let end_byte = chars.get(end).map_or(limit, |&(p, _)| p);
                result.push(chars[i].0..end_byte);
                i = end;
            }
            None => i += 1,
        }
    }
    result
}

fn at(chars: &[(usize, char)], i: usize) -> Option<char> {
    chars.get(i).map(|&(_, c)| c)
}

fn email(chars: &[(usize, char)], start: usize, previous: Option<char>) -> Option<usize> {
    if !at(chars, start).is_some_and(gfm_atext) || !previous_email(previous) {
        return None;
    }
    let mut i = start;
    while at(chars, i).is_some_and(gfm_atext) {
        i += 1;
    }
    if at(chars, i) != Some('@') {
        return None;
    }
    i += 1;
    let mut has_dot = false;
    loop {
        match at(chars, i) {
            Some('.') => {
                if punctuation(chars, i) {
                    break;
                }
                has_dot = true;
                i += 1;
            }
            Some('-' | '_') => {
                if punctuation(chars, i) {
                    return None;
                }
                i += 1;
                if at(chars, i) == Some('.') {
                    if punctuation(chars, i) {
                        return None;
                    }
                    has_dot = true;
                    i += 1;
                }
            }
            Some(c) if c.is_ascii_alphanumeric() => i += 1,
            _ => break,
        }
    }
    has_dot.then_some(i)
}

fn http(chars: &[(usize, char)], start: usize, previous: Option<char>) -> Option<usize> {
    if !at(chars, start).is_some_and(|c| c.eq_ignore_ascii_case(&'h')) || !previous_http(previous) {
        return None;
    }
    let mut i = start + 1;
    for want in ['t', 't', 'p'] {
        if !at(chars, i).is_some_and(|c| c.eq_ignore_ascii_case(&want)) {
            return None;
        }
        i += 1;
    }
    if at(chars, i).is_some_and(|c| c.eq_ignore_ascii_case(&'s')) {
        i += 1;
    }
    for want in [':', '/', '/'] {
        if at(chars, i) != Some(want) {
            return None;
        }
        i += 1;
    }
    match at(chars, i) {
        Some(c) if !ascii_control(c) && !unicode_whitespace(c) && !unicode_punctuation(c) => {}
        _ => return None,
    }
    domain(chars, i).map(|i| path(chars, i))
}

fn www(chars: &[(usize, char)], start: usize, previous: Option<char>) -> Option<usize> {
    if !previous_www(previous) {
        return None;
    }
    for k in 0..3 {
        if !at(chars, start + k).is_some_and(|c| c.eq_ignore_ascii_case(&'w')) {
            return None;
        }
    }
    if at(chars, start + 3) != Some('.') || at(chars, start + 4).is_none_or(line_ending) {
        return None;
    }
    // GitHub と同じく、www. も domain の一部として読む
    domain(chars, start).map(|i| path(chars, i))
}

fn domain(chars: &[(usize, char)], start: usize) -> Option<usize> {
    let mut i = start;
    let mut underscore_last = false;
    let mut underscore_last_last = false;
    while let Some(c) = at(chars, i) {
        if c == '&' {
            if named_character_reference(chars, i) {
                break;
            }
        } else if c == '.' || c == '_' {
            if punctuation(chars, i) {
                break;
            }
            if c == '.' {
                underscore_last_last = underscore_last;
                underscore_last = false;
            } else {
                underscore_last = true;
            }
        } else if ascii_control(c) || unicode_whitespace(c) || (c != '-' && unicode_punctuation(c))
        {
            break;
        }
        i += 1;
    }
    (!underscore_last && !underscore_last_last).then_some(i)
}

fn path(chars: &[(usize, char)], start: usize) -> usize {
    let mut i = start;
    let mut balance = 0i32;
    loop {
        let Some(c) = at(chars, i) else { return i };
        match c {
            '&' if named_character_reference(chars, i) => return i,
            '(' => balance += 1,
            ')' if punctuation(chars, i) => {
                balance -= 1;
                if balance < 0 {
                    return i;
                }
            }
            _ if path_end(Some(c)) => return i,
            _ if trailing_punctuation(c) && punctuation(chars, i) => return i,
            _ => {}
        }
        i += 1;
    }
}

/// syntax.js の tokenizeNamedCharacterReference: `&` と英字と `;` の後が、パスの終わり
fn named_character_reference(chars: &[(usize, char)], start: usize) -> bool {
    let mut i = start + 1;
    while at(chars, i).is_some_and(|c| c.is_ascii_alphabetic()) {
        i += 1;
    }
    at(chars, i) == Some(';') && path_end(at(chars, i + 1))
}

/// syntax.js の tokenizePunctuation: この文字と、続く末尾の句読点の後が、パスの終わり
fn punctuation(chars: &[(usize, char)], start: usize) -> bool {
    let mut i = start + 1;
    while at(chars, i).is_some_and(trailing_punctuation) {
        i += 1;
    }
    path_end(at(chars, i))
}

fn trailing_punctuation(c: char) -> bool {
    matches!(
        c,
        '!' | '"' | '\'' | ')' | '*' | ',' | '.' | ':' | ';' | '<' | '?' | '_' | '~'
    )
}

/// 文の終わり、行の終わり、tab、空白、`<`（micromark の負の code は行の終わりと tab）
fn path_end(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => matches!(c, '\n' | '\r' | '\t' | ' ' | '<'),
    }
}

fn line_ending(c: char) -> bool {
    matches!(c, '\n' | '\r')
}

fn gfm_atext(c: char) -> bool {
    matches!(c, '+' | '-' | '.' | '_') || c.is_ascii_alphanumeric()
}

fn previous_www(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => matches!(c, '\n' | '\r' | '\t' | ' ' | '(' | '*' | '_' | '~'),
    }
}

fn previous_http(c: Option<char>) -> bool {
    !c.is_some_and(|c| c.is_ascii_alphabetic())
}

fn previous_email(c: Option<char>) -> bool {
    c != Some('/') && previous_http(c)
}

/// micromark の asciiControl（tab と行の終わりを含む）
fn ascii_control(c: char) -> bool {
    (c as u32) < 32 || c == '\u{7f}'
}

/// micromark の unicodeWhitespace（JavaScript の `\s`）
pub(crate) fn unicode_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// micromark の unicodePunctuation（ASCII の句読点と記号、Unicode の P の一般カテゴリ）
pub(crate) fn unicode_punctuation(c: char) -> bool {
    static PUNCTUATION: OnceLock<Regex> = OnceLock::new();
    if c.is_ascii() {
        return c.is_ascii_punctuation();
    }
    PUNCTUATION
        .get_or_init(|| Regex::with_flags(r"^\p{P}$", "u").expect("固定の正規表現"))
        .find(c.encode_utf8(&mut [0; 4]))
        .is_some()
}
