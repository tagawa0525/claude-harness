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
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
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
        todo!()
    }
}

/// 前付けのキーと値（書かれた順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    entries: Vec<(String, Value)>,
}

impl FrontMatter {
    /// キーの値。キーがなければ `None`。
    pub fn get(&self, _key: &str) -> Option<&Value> {
        todo!()
    }

    /// 書かれた順のキー。
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }
}

/// Markdown の先頭の `---` で囲んだ前付けを読む。
pub fn parse_front_matter(_text: &str) -> Result<FrontMatter, Error> {
    todo!()
}

/// `ADR-NNNN`（4 桁）の参照の番号を、現れた順に返す。
pub fn adr_refs(_text: &str) -> Vec<u32> {
    todo!()
}

/// Markdown の、コードブロックの外の行を、行番号（1 から）とともに返す。
pub fn markdown_lines(_text: &str) -> Result<Vec<(usize, &str)>, Error> {
    todo!()
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
