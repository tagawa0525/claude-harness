//! 崩れた日本語の語の辞書（prh.yml）を読み、prh 6.0.0 と同じ意味で文に当てる（ADR-0085）。

use std::fmt;
use std::path::Path;

/// 辞書を読めない、prh が読めない書き方がある、specs が合わない
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryError(pub String);

/// 文に、prh と同じ意味で当てられない箇所がある
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError(pub String);

impl fmt::Display for DictionaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DictionaryError {}
impl std::error::Error for TextError {}

/// 文の中の 1 つの直し（位置は文の UTF-8 のバイト）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// 辞書の規則の並び
#[derive(Debug)]
pub struct Dictionary {}

impl Dictionary {
    /// 辞書のファイルを順に読み、規則を並べる
    pub fn load<P: AsRef<Path>>(_paths: &[P]) -> Result<Self, DictionaryError> {
        Err(DictionaryError("unimplemented".into()))
    }

    /// 辞書の YAML を読む。name は理由の表示に使う
    pub fn parse(_yaml: &str, _name: &str) -> Result<Self, DictionaryError> {
        Err(DictionaryError("unimplemented".into()))
    }

    pub fn len(&self) -> usize {
        0
    }

    pub fn is_empty(&self) -> bool {
        true
    }

    /// text を段落に分けて規則を当て、重なりを外した直しを先頭から並べる
    pub fn diffs(&self, _text: &str) -> Result<Vec<Diff>, TextError> {
        Ok(Vec::new())
    }

    /// text に規則を当てた結果
    pub fn apply(&self, text: &str) -> Result<String, TextError> {
        Ok(text.to_string())
    }
}
