//! 最後の応答が日本語で書かれているかの判定（`claude-hooks stop` が使う。ADR-0108）。
//!
//! Markdown の本文から、コードブロック、インラインのコード、引用のブロック、HTML を除いた文を数える。
//! - 英語の語: 空白と ASCII でない文字で区切り、両端の ASCII の記号を除いた語のうち、ASCII の英字と、その間の `'` と
//!   `-` だけでできたもの。パス、`snake_case`、`file.ext`、URL、数字を含む語は数えない
//! - 日本語の文字: ひらがな、カタカナ、漢字
//!
//! 英語の語が [`MIN_ENGLISH_WORDS`] 以上で、日本語の文字が [`MIN_JAPANESE_CHARS`] 未満なら英語と見る。誤って止めると
//! 応答の終わりごとに手戻りが出るので、日本語の文字が少しでもまとまってあれば通す側に倒す

/// これより英語の語が少ない応答は止めない
pub const MIN_ENGLISH_WORDS: usize = 15;
/// これだけ日本語の文字があれば、英語の語が多くても止めない
pub const MIN_JAPANESE_CHARS: usize = 20;

/// 数えた結果
#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub english_words: usize,
    pub japanese_chars: usize,
}

impl Counts {
    pub fn is_english(&self) -> bool {
        todo!("ADR-0108 の判定は次のコミットで足す")
    }
}

pub fn count(_message: &str) -> Counts {
    todo!("ADR-0108 の数え方は次のコミットで足す")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(message: &str) -> usize {
        count(message).english_words
    }

    #[test]
    #[ignore = "RED: 数え方（ADR-0108）は次のコミットで足す"]
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
    #[ignore = "RED: 数え方（ADR-0108）は次のコミットで足す"]
    fn does_not_count_code_blocks_inline_code_or_block_quotes() {
        let message = "```\nlet words = many more english words here\n```\n\n\
                       ~~~text\nanother fenced block of words\n~~~\n\n    indented code words\n\n\
                       > a quoted english passage with words\n\n`inline code words` end";
        assert_eq!(words(message), 1);
    }

    #[test]
    #[ignore = "RED: 数え方（ADR-0108）は次のコミットで足す"]
    fn does_not_count_paths_identifiers_urls_or_numbers() {
        let message = "crates/claude-hooks/src/stop.rs C:\\Users\\u snake_case file.ext \
                       https://example.com/a v1 0x1f serde_json::Value";
        assert_eq!(words(message), 0);
    }

    #[test]
    #[ignore = "RED: 数え方（ADR-0108）は次のコミットで足す"]
    fn counts_words_joined_by_apostrophes_and_hyphens_and_wrapped_in_punctuation() {
        assert_eq!(words("I've (re-run) the \"checks\", **twice**: don't."), 6);
    }

    #[test]
    #[ignore = "RED: 数え方（ADR-0108）は次のコミットで足す"]
    fn an_unclosed_fence_hides_the_rest() {
        assert_eq!(words("```\nsome words that never close"), 0);
    }

    #[test]
    #[ignore = "RED: 数え方（ADR-0108）は次のコミットで足す"]
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
