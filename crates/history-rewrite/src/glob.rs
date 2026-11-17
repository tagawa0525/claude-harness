//! `--exclude` の glob。Python の `PurePosixPath(path).full_match(pattern)` と同じパスに当てる（ADR-0088 の案 は）。
//!
//! 照合の作り方は Python 3.14 の `glob.translate(pattern, recursive=True, include_hidden=True, seps="/")` と
//! `fnmatch._translate`（pathlib の `_StringGlobber.compile` が full_match に使う）に揃えたもの:
//!
//! - pattern を `/` で成分に分ける。成分の全体の `*` は 1 つの成分、`**` は 0 個以上の成分に当たる
//! - ほかの成分の `*` と `?` は `/` をまたがず、`[…]` は文字クラス（`[!…]` は否定）。`{a,b}` の選択と `\` の
//!   エスケープは持たない（どちらもただの字）
//! - 隠れた名前（`.` で始まる成分）にも当たる。大文字と小文字は区別する
//!
//! path と pattern は先に PurePosixPath として読み直す（`.` の成分、重なった `/`、末尾の `/` を外す）。

use regex::Regex;

/// path が pattern の全体に当たるか
pub fn full_match(path: &str, pattern: &str) -> bool {
    Glob::new(pattern).matches(path)
}

/// 正規表現に直した glob
#[derive(Debug, Clone)]
pub struct Glob(Regex);

impl Glob {
    pub fn new(pattern: &str) -> Glob {
        // translate は字を逃がし、クラスの中も Rust の regex の書き方にするので、読めない正規表現にならない
        Glob(Regex::new(&translate(&normalize(pattern))).expect("translate は読める正規表現を作る"))
    }

    /// path も PurePosixPath として読み直してから照合する
    pub fn matches(&self, path: &str) -> bool {
        self.0.is_match(&normalize(path))
    }
}

/// PurePosixPath の文字列の形（path にも pattern にも当てる）。先頭の `//` が 2 つちょうどなら残し、ほかの重なった `/` は 1 つにし、`.` の成分と末尾の
/// `/` を外す。成分が残らなければ空（full_match は空のパスを `.` でなく空として照合する）
fn normalize(pattern: &str) -> String {
    let root = if pattern.starts_with("//") && !pattern.starts_with("///") {
        "//"
    } else if pattern.starts_with('/') {
        "/"
    } else {
        ""
    };
    let parts: Vec<&str> = pattern
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    format!("{root}{}", parts.join("/"))
}

const ONE_SEGMENT: &str = "[^/]+/";
const ONE_LAST_SEGMENT: &str = "[^/]+";
const ANY_SEGMENTS: &str = "(?:.+/)?";
const ANY_LAST_SEGMENTS: &str = ".*";

/// glob.translate（recursive、include_hidden、区切りは `/`）
fn translate(pattern: &str) -> String {
    let parts: Vec<&str> = pattern.split('/').collect();
    let last = parts.len() - 1;
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        match *part {
            "*" => out.push_str(if i < last {
                ONE_SEGMENT
            } else {
                ONE_LAST_SEGMENT
            }),
            "**" => {
                if i == last {
                    out.push_str(ANY_LAST_SEGMENTS);
                } else if parts[i + 1] != "**" {
                    out.push_str(ANY_SEGMENTS);
                }
            }
            _ => {
                translate_part(part, &mut out);
                if i < last {
                    out.push('/');
                }
            }
        }
    }
    format!(r"\A(?s:{out})\z")
}

/// fnmatch._translate（`*` は `[^/]*`、`?` は `[^/]`）
fn translate_part(part: &str, out: &mut String) {
    let p: Vec<char> = part.chars().collect();
    let n = p.len();
    let mut i = 0;
    while i < n {
        let c = p[i];
        i += 1;
        match c {
            '*' => {
                out.push_str("[^/]*");
                while i < n && p[i] == '*' {
                    i += 1;
                }
            }
            '?' => out.push_str("[^/]"),
            '[' => {
                let mut j = i;
                if j < n && p[j] == '!' {
                    j += 1;
                }
                if j < n && p[j] == ']' {
                    j += 1;
                }
                while j < n && p[j] != ']' {
                    j += 1;
                }
                if j >= n {
                    out.push_str(r"\[");
                    continue;
                }
                let stuff = class_body(&p, i, j);
                i = j + 1;
                if stuff.is_empty() {
                    // 空の範囲: どれにも当たらない（Python は (?!)。Rust の regex は先読みを持たないので、何にも当たらないクラス）
                    out.push_str(r"[^\x00-\x{10FFFF}]");
                } else if stuff == "!" {
                    // 否定の空の範囲: どの 1 字にも当たる
                    out.push('.');
                } else {
                    out.push('[');
                    out.push_str(&class_regex(&stuff));
                    out.push(']');
                }
            }
            _ => out.push_str(&regex::escape(&c.to_string())),
        }
    }
}

/// `[` の次（start）から `]`（j）の前までのうち、範囲の `-` を残し、ほかの `\` と `-` を逃がしたもの（fnmatch の
/// stuff。先頭の `!` は否定として残す）
fn class_body(p: &[char], start: usize, j: usize) -> String {
    let stuff: String = p[start..j].iter().collect();
    if !stuff.contains('-') {
        return stuff.replace('\\', r"\\");
    }
    let mut i = start;
    let mut chunks: Vec<Vec<char>> = Vec::new();
    let mut k = if p[i] == '!' { i + 2 } else { i + 1 };
    while let Some(found) = (k..j).find(|&x| p[x] == '-') {
        chunks.push(p[i..found].to_vec());
        i = found + 1;
        k = found + 3;
    }
    let chunk = p[i..j].to_vec();
    if chunk.is_empty() {
        if let Some(last) = chunks.last_mut() {
            last.push('-');
        }
    } else {
        chunks.push(chunk);
    }
    // 空の範囲（z-a のような逆の範囲）を外す
    let mut k = chunks.len();
    while k > 1 {
        k -= 1;
        if let (Some(&end), Some(&begin)) = (chunks[k - 1].last(), chunks[k].first())
            && end > begin
        {
            let mut joined = chunks[k - 1][..chunks[k - 1].len() - 1].to_vec();
            joined.extend_from_slice(&chunks[k][1..]);
            chunks[k - 1] = joined;
            chunks.remove(k);
        }
    }
    chunks
        .iter()
        .map(|c| {
            c.iter()
                .collect::<String>()
                .replace('\\', r"\\")
                .replace('-', r"\-")
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// 文字クラスの中身を Rust の regex の書き方にする。先頭の `!` は否定。Rust の regex のクラスで意味を持つ字（入れ子の
/// `[`、閉じの `]`、集合の演算の `&&` と `~~`、先頭の `^`）は逃がす（Python は `&`、`~`、`|` と先頭の `^` と `[` を逃がす）
fn class_regex(stuff: &str) -> String {
    let (negated, body) = match stuff.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, stuff),
    };
    let mut out = String::from(if negated { "^" } else { "" });
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            // class_body が逃がした `\\` と `\-` はそのまま
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if matches!(c, '[' | ']' | '&' | '~' | '|') || (c == '^' && out.is_empty()) {
            out.push('\\');
        }
        out.push(c);
        i += 1;
    }
    out
}
