//! block-windows-double-backslash: Windows で、Bash ツールの書き換えで bash の結果が変わるバックスラッシュの並びを止める
//!
//! Windows の Bash ツールは、コマンドの文字列のバックスラッシュの並び（2 つ以上）を、`"` の直前のものを除き、bash に
//! 渡す前に 2 つごとに 1 つにする（`a\\b` は `a\b`、`a\\\b` と `a\\\\b` は `a\\b`。`c\\"d` はそのまま）。hook には
//! 書いたとおりに届き、bash には書き換えた後で届くので、bash の引用の規則では防げない: 引用符つきのヒアドキュメント
//! （`<<'EOF'`）、引用符なしのヒアドキュメント、`python -c`、単引用符の引数のどれでも書き換わる。Windows のパスを含む
//! Python の文字列（`'C:\\Users'` が `'C:\Users'` になり unicodeescape で落ちる）や、ファイルに書いた本文が崩れる。
//!
//! 止めるのは、書き換えで bash の結果が変わる並びだけ。1 つだけのバックスラッシュと `"` の直前の並びは書き換わらない。
//! 二重引用符の文字列と引用符なしのヒアドキュメントの本文の地の文（展開とコマンド置換の外）の、ちょうど 2 つの並びは、
//! bash が特別に読む文字（`$`、`` ` ``、改行）の前でなければ、1 つにされても bash が同じ 1 つにするので通す
//! （`grep "foo\\|bar"`）。地の文かは tree-sitter の木で決め（`Shell::literal_at`）、木に構文の誤りがあるときと、
//! ヒアドキュメントの本文が bash の読む本文（`Shell::heredocs`）と合わないときは通さない。手で引用を追う走査にしない
//! （`case` の `)`、`${…}`、本文の中の `$(…)` で読み違えた。#437）。バッククォートの中とコメントは、2 つでも止める
//! （バッククォートの中の読み方を追わない。tree-sitter はエスケープした空白の後の `#` をコメントと読む）。
//! Windows でない機械では止めない（Linux の Bash ツールは確かめていない）。エスケープは置かない（止めるときは、
//! 書いたとおりに届かないことが決まっている）。止めずに入力を書き換えて並びを倍にしない（hook の後の書き換えは、
//! Bash ツールの公開されない挙動に頼る。直ると、倍にした並びがそのまま届いて壊れる。#437）

use std::ops::Range;

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Literal, Shell};

pub struct BlockWindowsDoubleBackslash;

impl Rule for BlockWindowsDoubleBackslash {
    fn name(&self) -> &'static str {
        "block-windows-double-backslash"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        if cfg!(windows) && changes_result(shell) {
            vec![Finding::Deny(DENY.to_string())]
        } else {
            Vec::new()
        }
    }
}

/// bash が読むヒアドキュメントの本文 1 つの、原文の中の位置
struct Body {
    /// 本文（閉じの行を除く）
    text: Range<usize>,
    /// 区切りを引用したか（本文を bash が読まない）
    quoted: bool,
}

/// 原文の中の位置を決められるヒアドキュメントの本文。`bash -c` の文字列の中のもの（位置が原文の外）は除く
fn heredoc_bodies(shell: &Shell) -> Vec<Body> {
    let src = shell.src();
    let base = src.as_ptr() as usize;
    let offset = |s: &str| {
        (s.as_ptr() as usize)
            .checked_sub(base)
            .filter(|o| *o <= src.len())
    };
    shell
        .heredocs()
        .iter()
        .filter_map(|span| {
            let line = offset(span.line)?;
            // 閉じの行で終わるコマンド（末尾の改行が無い）の rest は、原文の外の空の文字列
            let resume = if span.rest.is_empty() {
                src.len()
            } else {
                offset(span.rest)?
            };
            let start = (line + span.line.len() + 1).min(resume);
            // 閉じの行の始まり（その前の改行の次）。閉じの行が多バイトの文字で終わりうるので、バイトで探す
            let end = src.as_bytes()[..resume.saturating_sub(1)]
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(start, |i| i + 1)
                .max(start);
            Some(Body {
                text: start..end,
                quoted: span.heredoc.quoted,
            })
        })
        .collect()
}

/// bash が二重引用符の中と引用符なしのヒアドキュメントで、バックスラッシュの後ろを特別に読む文字（`"` は書き換わらない
/// ので除く）。None はコマンドの終わり
fn special_after_backslash(c: Option<u8>) -> bool {
    matches!(c, None | Some(b'$' | b'`' | b'\n'))
}

/// Windows の Bash ツールの書き換えで、bash の結果が変わる並びがあるか
fn changes_result(shell: &Shell) -> bool {
    let bodies = heredoc_bodies(shell);
    runs(shell.src().as_bytes())
        .into_iter()
        .any(|(at, n, next)| !same_after_rewrite(shell, &bodies, at..at + n, next))
}

/// 書き換わる並び（at..at+n。次の文字は next）を 1 つにされても、bash が同じ結果にするか
fn same_after_rewrite(shell: &Shell, bodies: &[Body], run: Range<usize>, next: Option<u8>) -> bool {
    if run.len() != 2 || special_after_backslash(next) {
        return false;
    }
    let body = bodies.iter().find(|b| b.text.contains(&run.start));
    match (shell.literal_at(run.clone()), body) {
        (Literal::DoubleQuoted, None) => true,
        (Literal::HeredocBody, Some(body)) => !body.quoted && run.end <= body.text.end,
        _ => false,
    }
}

/// Bash ツールが書き換える並び（2 つ以上で、`"` の直前でない）の（位置, 長さ, 次の文字）
fn runs(src: &[u8]) -> Vec<(usize, usize, Option<u8>)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < src.len() {
        if src[i] != b'\\' {
            i += 1;
            continue;
        }
        let n = src[i..].iter().take_while(|c| **c == b'\\').count();
        let next = src.get(i + n).copied();
        if n >= 2 && next != Some(b'"') {
            found.push((i, n, next));
        }
        i += n;
    }
    found
}

const DENY: &str = r#"コマンドに、Windows の Bash ツールが書き換えて bash の結果が変わるバックスラッシュの並びがあります。Bash ツールは、" の直前でないバックスラッシュの並びを、bash に渡す前に 2 つごとに 1 つにします（\\ が \ に。単引用符、引用符つきのヒアドキュメント <<'EOF'、引用符の外、$(…) の中のどれでも）。書いたとおりに届かず、Windows のパスを含む Python の文字列（'C:\\Users' が 'C:\Users' になり unicodeescape で落ちる）や、ファイルに書いた本文が崩れるので止めます（二重引用符の中の 2 つは、$ ` 改行の前でなければ bash も 1 つにするので止めません）。
代わりに:
- バックスラッシュを含むスクリプトやメッセージは、Write でファイルに書いてから、ファイルで渡す（python <ファイル>、git commit -F <ファイル>。Write はバックスラッシュを書いたとおりに残します）
- Windows のパスは / で区切る（C:/Users/...。Python も Git Bash も受けます）
- 正規表現などでバックスラッシュが 1 つ要るだけなら、1 つで書く（1 つは書いたとおりに届きます）"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(src: &str) -> bool {
        changes_result(&Shell::parse(src))
    }

    #[test]
    fn a_run_outside_double_quotes_changes_the_result() {
        for src in [
            r"echo 'a\\b'",
            r"echo a\\b",
            r"echo 'a\\\b'",
            r"echo $'a\\b'",
            r"sed 's/\\/\//g' f",
            r#"python -c 'print(r"C:\\Users")'"#,
            "python - <<'EOF'\nprint(r'C:\\\\Users')\nEOF",
            "echo `printf 'a\\\\b'`",
        ] {
            assert!(changes(src), "{src}");
        }
    }

    #[test]
    fn a_run_in_double_quotes_changes_the_result_before_a_special_character_or_when_longer() {
        for src in [
            r#"echo "a\\$HOME""#,
            r#"echo "a\\`date`""#,
            r#"echo "a\\\x""#,
            r#"echo "a\\\\x""#,
            "echo \"a\\\\\nb\"",
            r#"x="$(grep 'a\\b' f)""#,
            r#"x="$(echo "$(printf a\\b)")""#,
        ] {
            assert!(changes(src), "{src}");
        }
    }

    #[test]
    fn an_unquoted_heredoc_body_reads_like_double_quotes() {
        assert!(!changes("cat <<EOF\nfoo\\\\|bar\nEOF"));
        assert!(changes("cat <<EOF\nC:\\\\\\\\Users\nEOF"));
        assert!(changes("cat <<EOF\na\\\\$HOME\nEOF"));
        assert!(changes("cat <<'EOF'\nfoo\\\\|bar\nEOF"));
        assert!(changes(
            "git commit -F - <<EOF\nfoo\\\\|bar\nEOF\necho 'a\\\\b'"
        ));
    }

    #[test]
    fn runs_that_reach_bash_unchanged_or_mean_the_same_pass() {
        for src in [
            r"echo 'a\b'",
            r"grep '\.' f",
            r#"printf '%s\n' 'c\\"d'"#,
            r#"bash -c "echo \\\"hi\\\"""#,
            r#"grep -n "foo\\|bar" f"#,
            r#"python -c "print(r'C:\\Users')""#,
            r#"echo "a\\.b" "$(echo x)" "c\\d""#,
            "echo a",
        ] {
            assert!(!changes(src), "{src}");
        }
    }

    /// 二重引用符の中でも、展開（`${…}`）とコマンド置換（`case` の `)` を含む）の中は、bash が引用符の外と同じに読む
    #[test]
    fn a_run_inside_an_expansion_in_double_quotes_changes_the_result() {
        for src in [
            r#"p='C:\Users/x'; echo "${p//\\//}""#,
            r#"x="$(case a in a) printf '%s' 'a\\b';; esac)""#,
            "cat <<EOF\n$(printf '%s' 'a\\\\b')\nEOF",
        ] {
            assert!(changes(src), "{src}");
        }
    }

    /// 本文の位置を決められないヒアドキュメントと、コメントらしく見える語の中は、二重引用符の中として通さない
    #[test]
    fn a_run_whose_quoting_is_not_certain_is_not_passed() {
        for src in [
            "cat <<A <<'B'\nx\nA\nprint(\"C:\\\\Users\")\nB",
            "echo \\ #'a\\\\b'",
            "# a\\\\b\necho a",
        ] {
            assert!(changes(src), "{src}");
        }
    }

    /// 区切りが ASCII でないヒアドキュメントは、tree-sitter が構文の誤りとして読むので、本文の 2 つも通さない（閉じの
    /// 行が多バイトの文字で終わっても panic しない）
    #[test]
    fn a_heredoc_with_a_non_ascii_delimiter_is_not_passed() {
        assert!(changes("cat <<終\nfoo\\\\|bar\n終"));
        assert!(changes("cat <<'終'\nC:\\\\Users\n終"));
    }

    #[test]
    fn an_unclosed_quote_does_not_pass_a_run_read_as_double_quoted() {
        assert!(changes(r#"echo "a\\b"'"#));
        assert!(changes("echo \"a\\\\b"));
    }
}
