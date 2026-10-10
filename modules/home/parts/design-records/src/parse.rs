//! Markdown の前付け、コードブロックの外の行、`ADR-NNNN` の参照を読む。

use std::fmt;

/// 読めなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 先頭の行が `---` でない。
    MissingFrontMatter,
    /// 前付けを閉じる `---` がない。
    UnclosedFrontMatter,
    /// `key: value` の形でない行（前付けの中の行番号は 1 から）。
    MalformedLine { line: usize, text: String },
    /// 同じキーが 2 回ある。
    DuplicateKey(String),
    /// 閉じていない引用符や角括弧。
    MalformedValue { key: String, value: String },
    /// 閉じていないコードブロック（開いた行の番号は 1 から）。
    UnclosedFence { line: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MissingFrontMatter => {
                write!(f, "the file does not start with a `---` front matter")
            }
            Error::UnclosedFrontMatter => write!(f, "the front matter has no closing `---`"),
            Error::MalformedLine { line, text } => {
                write!(f, "front matter line {line} is not `key: value`: {text:?}")
            }
            Error::DuplicateKey(key) => write!(f, "front matter key `{key}` appears twice"),
            Error::MalformedValue { key, value } => {
                write!(f, "front matter value of `{key}` is malformed: {value:?}")
            }
            Error::UnclosedFence { line } => {
                write!(f, "code fence opened on line {line} is not closed")
            }
        }
    }
}

impl std::error::Error for Error {}

/// 前付けの値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// `key:` のあとが空。
    Empty,
    /// 裸の文字列か、引用符で囲んだ文字列（引用符は外す）。
    Scalar(String),
    /// `[a, "b"]` の形の一覧（各要素の引用符は外す）。
    List(Vec<String>),
}

impl Value {
    /// 値に含まれる文字列。空なら空、1 つの文字列なら 1 つ、一覧ならその要素。
    pub fn items(&self) -> Vec<&str> {
        match self {
            Value::Empty => Vec::new(),
            Value::Scalar(s) => vec![s.as_str()],
            Value::List(items) => items.iter().map(String::as_str).collect(),
        }
    }
}

/// 前付けのキーと値（書かれた順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    entries: Vec<(String, Value)>,
}

impl FrontMatter {
    /// キーの値。キーがなければ `None`。
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// 書かれた順のキー。
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }
}

/// Markdown の先頭の `---` で囲んだ前付けを読む。
///
/// 各行は `key: value` で、値は空、裸の文字列、引用符で囲んだ文字列、`[a, "b"]` の一覧のどれか。`#` は値の
/// 一部として読む（YAML のコメントとして外すと、`"#12"` を引用符なしで書いたときに値が消える）。空行、重複した
/// キー、閉じていない引用符や角括弧はエラーにする。
pub fn parse_front_matter(text: &str) -> Result<FrontMatter, Error> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err(Error::MissingFrontMatter);
    }
    let mut entries: Vec<(String, Value)> = Vec::new();
    for (i, line) in lines.enumerate() {
        if line == "---" {
            return Ok(FrontMatter { entries });
        }
        let malformed = || Error::MalformedLine {
            line: i + 1,
            text: line.to_string(),
        };
        let (key, value) = line.trim_end().split_once(':').ok_or_else(malformed)?;
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err(malformed());
        }
        if entries.iter().any(|(k, _)| k == key) {
            return Err(Error::DuplicateKey(key.to_string()));
        }
        let value = parse_value(key, value.trim())?;
        entries.push((key.to_string(), value));
    }
    Err(Error::UnclosedFrontMatter)
}

fn parse_value(key: &str, value: &str) -> Result<Value, Error> {
    let malformed = || Error::MalformedValue {
        key: key.to_string(),
        value: value.to_string(),
    };
    if value.is_empty() {
        return Ok(Value::Empty);
    }
    if let Some(rest) = value.strip_prefix('[') {
        let inner = rest.strip_suffix(']').ok_or_else(malformed)?.trim();
        if inner.is_empty() {
            return Ok(Value::List(Vec::new()));
        }
        return inner
            .split(',')
            .map(|item| match unquote(item.trim()) {
                Some(s) if !s.is_empty() => Ok(s.to_string()),
                _ => Err(malformed()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List);
    }
    unquote(value)
        .map(|s| Value::Scalar(s.to_string()))
        .ok_or_else(malformed)
}

/// `"text"` なら中身を、引用符を含まない裸の文字列ならそのままを返す。引用符が対にならなければ `None`。
fn unquote(s: &str) -> Option<&str> {
    let inner = match s.strip_prefix('"') {
        Some(rest) => rest.strip_suffix('"')?,
        None => s,
    };
    (!inner.contains('"')).then_some(inner)
}

/// 英数字か `_`（ASCII のみ。日本語の文に続けて書いた参照は、参照として数える）。
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `text[start..]` の先頭がちょうど 4 桁の数字で、そのあとに英数字が続かなければ、その数を返す。
fn four_digits_at(text: &str, start: usize) -> Option<u32> {
    let digits = text.get(start..start + 4)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if text[start + 4..].chars().next().is_some_and(is_word) {
        return None;
    }
    digits.parse().ok()
}

/// `ADR-NNNN`（4 桁）の参照の番号を、現れた順に返す。
pub fn adr_refs(text: &str) -> Vec<u32> {
    const PREFIX: &str = "ADR-";
    text.match_indices(PREFIX)
        .filter(|(i, _)| !text[..*i].chars().next_back().is_some_and(is_word))
        .filter_map(|(i, _)| four_digits_at(text, i + PREFIX.len()))
        .collect()
}

/// 4 つ以上の空白で字下げしていない行の、字下げを除いた残り。
fn unindented(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(' ');
    (line.len() - rest.len() <= 3).then_some(rest)
}

/// 先頭に続く `c` の数。
fn run_of(text: &str, c: char) -> usize {
    text.len() - text.trim_start_matches(c).len()
}

/// コードブロックを開く囲い（CommonMark）の記号と数。3 つ以上の ```` ` ```` か `~` で、字下げは 3 つまで。
/// ```` ` ```` の囲いの情報の文字列にはバッククォートを書けない（```` ```cargo test``` ```` は囲いでない）。
fn opening_fence(line: &str) -> Option<(char, usize)> {
    let rest = unindented(line)?;
    ['`', '~'].into_iter().find_map(|c| {
        let n = run_of(rest, c);
        (n >= 3 && !(c == '`' && rest[n..].contains('`'))).then_some((c, n))
    })
}

/// `c` が `n` 個以上並び、ほかに何もない、字下げが 3 つまでの行か（閉じる囲い）。
fn closes_fence(line: &str, c: char, n: usize) -> bool {
    unindented(line).is_some_and(|rest| {
        let run = run_of(rest, c);
        run >= n && rest[run..].trim().is_empty()
    })
}

/// Markdown の、コードブロック（CommonMark の ```` ``` ```` か `~~~` の囲い）の外の行を、行番号（1 から）とともに
/// 返す。前付けは読み分けない（ほかの行と同じに返す）。閉じていないコードブロックはエラーにする。
pub fn markdown_lines(text: &str) -> Result<Vec<(usize, &str)>, Error> {
    let mut out = Vec::new();
    // 開いているコードブロックの記号、数、開いた行
    let mut fence: Option<(char, usize, usize)> = None;
    for (i, line) in text.lines().enumerate() {
        let number = i + 1;
        match fence {
            None => match opening_fence(line) {
                Some((c, n)) => fence = Some((c, n, number)),
                None => out.push((number, line)),
            },
            Some((c, n, _)) => {
                if closes_fence(line, c, n) {
                    fence = None;
                }
            }
        }
    }
    match fence {
        Some((_, _, line)) => Err(Error::UnclosedFence { line }),
        None => Ok(out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(s: &str) -> Value {
        Value::Scalar(s.to_string())
    }

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn reads_front_matter_values() {
        let text = "---\n\
                    status: superseded\n\
                    date: 2026-10-08\n\
                    supersedes: [ADR-0002, ADR-0003]\n\
                    superseded-by: ADR-0007\n\
                    issues: [\"#12\"]\n\
                    requires:\n\
                    title: \"quoted text\"\n\
                    ---\n\
                    \n\
                    # ADR-0001: title\n";
        let fm = parse_front_matter(text).unwrap();
        assert_eq!(fm.get("status"), Some(&scalar("superseded")));
        assert_eq!(fm.get("date"), Some(&scalar("2026-10-08")));
        assert_eq!(fm.get("supersedes"), Some(&list(&["ADR-0002", "ADR-0003"])));
        assert_eq!(fm.get("superseded-by"), Some(&scalar("ADR-0007")));
        assert_eq!(fm.get("issues"), Some(&list(&["#12"])));
        assert_eq!(fm.get("requires"), Some(&Value::Empty));
        assert_eq!(fm.get("title"), Some(&scalar("quoted text")));
        assert_eq!(fm.get("missing"), None);
        assert_eq!(
            fm.keys().collect::<Vec<_>>(),
            [
                "status",
                "date",
                "supersedes",
                "superseded-by",
                "issues",
                "requires",
                "title"
            ]
        );
    }

    #[test]
    fn keeps_hashes_in_values() {
        let fm =
            parse_front_matter("---\nstatus: accepted # note\nissues: [\"#1\", \"#14\"]\n---\n")
                .unwrap();
        assert_eq!(fm.get("status"), Some(&scalar("accepted # note")));
        assert_eq!(fm.get("issues"), Some(&list(&["#1", "#14"])));
    }

    #[test]
    fn value_items_flatten_scalars_and_lists() {
        assert!(Value::Empty.items().is_empty());
        assert_eq!(scalar("a").items(), ["a"]);
        assert_eq!(list(&["a", "b"]).items(), ["a", "b"]);
    }

    #[test]
    fn rejects_malformed_front_matter() {
        assert_eq!(
            parse_front_matter("# title\n"),
            Err(Error::MissingFrontMatter)
        );
        assert_eq!(
            parse_front_matter("---\nstatus: open\n"),
            Err(Error::UnclosedFrontMatter)
        );
        assert!(matches!(
            parse_front_matter("---\nno colon here\n---\n"),
            Err(Error::MalformedLine { line: 1, .. })
        ));
        assert!(matches!(
            parse_front_matter("---\nstatus: open\n\n---\n"),
            Err(Error::MalformedLine { line: 2, .. })
        ));
        assert_eq!(
            parse_front_matter("---\nstatus: open\nstatus: closed\n---\n"),
            Err(Error::DuplicateKey("status".to_string()))
        );
        assert!(matches!(
            parse_front_matter("---\nissues: [\"x\"\n---\n"),
            Err(Error::MalformedValue { .. })
        ));
        assert!(matches!(
            parse_front_matter("---\ntitle: \"open quote\n---\n"),
            Err(Error::MalformedValue { .. })
        ));
    }

    #[test]
    fn errors_name_the_problem() {
        assert_eq!(
            Error::DuplicateKey("status".into()).to_string(),
            "front matter key `status` appears twice"
        );
        assert_eq!(
            Error::UnclosedFence { line: 3 }.to_string(),
            "code fence opened on line 3 is not closed"
        );
    }

    #[test]
    fn finds_adr_references() {
        assert_eq!(
            adr_refs("ADR-0002 と [ADR-0068](0068-x.md)、(ADR-0093)、ADR-0009 の"),
            [2, 68, 93, 9]
        );
        // 4 桁でないもの、語の途中のもの、書式の説明の `ADR-NNNN` は数えない
        assert!(adr_refs("ADR-NNNN ADR-001 ADR-00012 XADR-0001 ADR-0001x").is_empty());
    }

    #[test]
    fn skips_fences_but_not_front_matter() {
        let text = "---\n\
                    status: open\n\
                    ---\n\
                    one\n\
                    ```markdown\n\
                    hidden\n\
                    ~~~\n\
                    still hidden\n\
                    ```\n\
                    two\n\
                    ~~~~\n\
                    hidden\n\
                    ```\n\
                    ~~~~~\n\
                    three\n";
        assert_eq!(
            markdown_lines(text).unwrap(),
            [
                (1, "---"),
                (2, "status: open"),
                (3, "---"),
                (4, "one"),
                (10, "two"),
                (15, "three")
            ]
        );
    }

    #[test]
    fn follows_commonmark_fences() {
        // 囲いの記号のあとにバッククォートを含む行は、コードブロックを開かない
        assert_eq!(
            markdown_lines("```cargo test``` で走る\nx\n").unwrap(),
            [(1, "```cargo test``` で走る"), (2, "x")]
        );
        // 4 つ以上の空白で字下げした囲いは、囲いでない
        assert_eq!(
            markdown_lines("    ```\nx\n").unwrap(),
            [(1, "    ```"), (2, "x")]
        );
        assert!(markdown_lines("   ```\nx\n   ```\n").unwrap().is_empty());
        // 閉じる囲いは、同じ記号が開いたときと同じ数以上並び、ほかに何もない行
        assert_eq!(
            markdown_lines("```\n``` not closing\n```\ny\n").unwrap(),
            [(4, "y")]
        );
        // `~~~` の情報の文字列にはバッククォートを書ける
        assert_eq!(
            markdown_lines("~~~ info with ` tick\nx\n~~~\ny\n").unwrap(),
            [(4, "y")]
        );
    }

    #[test]
    fn rejects_unclosed_fences() {
        assert_eq!(
            markdown_lines("a\n\n```\ncode\n"),
            Err(Error::UnclosedFence { line: 3 })
        );
    }
}
