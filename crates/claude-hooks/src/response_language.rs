//! 最後の応答が日本語で書かれているかの判定（`claude-hooks stop` が使う。ADR-0108）。
//!
//! Markdown の本文から、コードブロック、インラインのコード、引用のブロック、HTML を除いた文を数える。
//! - 英語の語: 空白と ASCII でない文字で区切り、両端の ASCII の記号を除いた語のうち、ASCII の英字と、その間の `'` と
//!   `-` だけでできたもの。パス、`snake_case`、`file.ext`、URL、数字を含む語は数えない
//! - 日本語の文字: ひらがな、カタカナ、漢字
//!
//! 英語の語が [`MIN_ENGLISH_WORDS`] 以上で、日本語の文字が [`MIN_JAPANESE_CHARS`] 未満なら英語と見る。誤って止めると
//! 応答の終わりごとに手戻りが出るので、日本語の文字が少しでもまとまってあれば通す側に倒す

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// これより英語の語が少ない応答（「The checks passed. Waiting for the review.」のような 1 文）は止めない。手元の記録の
/// 877 件で、英語と見た 12 件はどれも 15 語以上だった（2026-11-27。ADR-0108）
pub const MIN_ENGLISH_WORDS: usize = 15;
/// これだけ日本語の文字があれば、英語の語が多くても止めない。同じ 877 件で、日本語の文字が 1 以上で 60 未満の応答は、
/// どれも英語の語が 8 未満だった
pub const MIN_JAPANESE_CHARS: usize = 20;

/// 数えた結果
#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub english_words: usize,
    pub japanese_chars: usize,
}

impl Counts {
    pub fn is_english(&self) -> bool {
        self.english_words >= MIN_ENGLISH_WORDS && self.japanese_chars < MIN_JAPANESE_CHARS
    }
}

pub fn count(message: &str) -> Counts {
    let prose = prose(message);
    Counts {
        english_words: prose
            .split(is_separator)
            .filter(|w| is_english_word(w))
            .count(),
        japanese_chars: prose.chars().filter(|c| is_japanese(*c)).count(),
    }
}

/// 数える文。pulldown-cmark は 1 つの語を記号の前後で複数の Text に分けて出すことがあるので、続く Text はつなぎ、
/// ほかの出来事（インラインのコード、改行、ブロックの境）は区切りにする
fn prose(message: &str) -> String {
    let mut out = String::new();
    let mut code = 0usize;
    let mut quote = 0usize;
    for event in Parser::new(message) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code += 1,
            Event::End(TagEnd::CodeBlock) => code = code.saturating_sub(1),
            Event::Start(Tag::BlockQuote(_)) => quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote = quote.saturating_sub(1),
            Event::Text(text) if code == 0 && quote == 0 => out.push_str(&text),
            Event::Text(_) => {}
            _ => out.push('\n'),
        }
    }
    out
}

fn is_separator(c: char) -> bool {
    c.is_whitespace() || !c.is_ascii()
}

fn is_english_word(word: &str) -> bool {
    let word = word.trim_matches(|c: char| c.is_ascii_punctuation());
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '\'' || c == '-')
}

/// ひらがな、カタカナ（長音符を含む）、CJK 統合漢字とその拡張 A、々
fn is_japanese(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '々')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(message: &str) -> usize {
        count(message).english_words
    }

    #[test]
    fn counts_plain_english_words_and_japanese_chars() {
        assert_eq!(
            count("The checks passed. 確認しました。"),
            Counts {
                english_words: 3,
                japanese_chars: 6
            }
        );
    }

    #[test]
    fn does_not_count_code_blocks_inline_code_or_block_quotes() {
        let message = "```\nlet words = many more english words here\n```\n\n\
                       ~~~text\nanother fenced block of words\n~~~\n\n    indented code words\n\n\
                       > a quoted english passage with words\n\n`inline code words` end";
        assert_eq!(words(message), 1);
    }

    #[test]
    fn does_not_count_paths_identifiers_urls_or_numbers() {
        let message = "crates/claude-hooks/src/stop.rs C:\\Users\\u snake_case file.ext \
                       https://example.com/a v1 0x1f serde_json::Value";
        assert_eq!(words(message), 0);
    }

    #[test]
    fn counts_words_joined_by_apostrophes_and_hyphens_and_wrapped_in_punctuation() {
        assert_eq!(words("I've (re-run) the \"checks\", **twice**: don't."), 6);
    }

    #[test]
    fn an_unclosed_fence_hides_the_rest() {
        assert_eq!(words("```\nsome words that never close"), 0);
    }

    #[test]
    fn english_needs_enough_words_and_little_japanese() {
        let english = "word ".repeat(MIN_ENGLISH_WORDS);
        assert!(count(&english).is_english());
        assert!(!count(&"word ".repeat(MIN_ENGLISH_WORDS - 1)).is_english());
        let japanese = "あ".repeat(MIN_JAPANESE_CHARS);
        assert!(!count(&format!("{japanese} {english}")).is_english());
        let some = "あ".repeat(MIN_JAPANESE_CHARS - 1);
        assert!(count(&format!("{some} {english}")).is_english());
    }
}
