//! block-windows-double-backslash: Windows で、Bash ツールの書き換えで bash の結果が変わるバックスラッシュの並びを止める
//!
//! Windows の Bash ツールは、コマンドの文字列のバックスラッシュの並び（2 つ以上）を、`"` の直前のものを除き、bash に
//! 渡す前に 2 つごとに 1 つにする（`a\\b` は `a\b`、`a\\\b` と `a\\\\b` は `a\\b`。`c\\"d` はそのまま）。hook には
//! 書いたとおりに届き、bash には書き換えた後で届くので、bash の引用の規則では防げない: 引用符つきのヒアドキュメント
//! （`<<'EOF'`）、引用符なしのヒアドキュメント、`python -c`、単引用符の引数のどれでも書き換わる。Windows のパスを含む
//! Python の文字列（`'C:\\Users'` が `'C:\Users'` になり unicodeescape で落ちる）や、ファイルに書いた本文が崩れる。
//!
//! 止めるのは、書き換えで bash の結果が変わる並びだけ。1 つだけのバックスラッシュと `"` の直前の並びは書き換わらない。
//! 二重引用符の中（`$(…)` と `` `…` `` の中を除く）と引用符なしのヒアドキュメントの本文の、ちょうど 2 つの並びは、
//! bash が特別に読む文字（`$`、`` ` ``、改行）の前でなければ、1 つにされても bash が同じ 1 つにするので通す
//! （`grep "foo\\|bar"`）。コメントの中は通す。引用の読み取りが閉じないまま終わったときは、二重引用符の中として通した
//! 並びも止める（読み違えた引用で通さない）。Windows でない機械では止めない（Linux の Bash ツールは確かめていない）。
//! エスケープは置かない（止めるときは、書いたとおりに届かないことが決まっている）。止めずに入力を書き換えて並びを
//! 倍にしない（hook の後の書き換えは、Bash ツールの公開されない挙動に頼る。直ると、倍にした並びがそのまま届いて
//! 壊れる。#437）

use std::ops::Range;

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct BlockWindowsDoubleBackslash;

impl Rule for BlockWindowsDoubleBackslash {
    fn name(&self) -> &'static str {
        "block-windows-double-backslash"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        if cfg!(windows) && changes_result(shell.src(), &heredoc_bodies(shell)) {
            vec![Finding::Deny(DENY.to_string())]
        } else {
            Vec::new()
        }
    }
}

/// ヒアドキュメントの本文 1 つの、原文の中の位置
struct Body {
    /// 本文（閉じの行を除く）
    text: Range<usize>,
    /// 閉じの行の後（走査を続ける位置）
    resume: usize,
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
            // 閉じの行の始まり（resume の前の改行の次）
            let end = src[..resume.saturating_sub(1)]
                .rfind('\n')
                .map_or(start, |i| i + 1)
                .max(start);
            Some(Body {
                text: start..end,
                resume,
                quoted: span.heredoc.quoted,
            })
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    /// 引用符の外
    Plain,
    /// `$(…)`（入れ子の括弧の深さ）
    Subst(u32),
    Single,
    /// `$'…'`
    AnsiC,
    Double,
    Backtick,
    Comment,
}

/// bash が二重引用符の中と引用符なしのヒアドキュメントで、バックスラッシュの後ろを特別に読む文字（`"` は書き換わらない
/// ので除く）
fn special_after_backslash(c: Option<u8>) -> bool {
    matches!(c, None | Some(b'$' | b'`' | b'\n'))
}

/// Windows の Bash ツールの書き換えで、bash の結果が変わる並びがあるか
fn changes_result(src: &str, bodies: &[Body]) -> bool {
    let bytes = src.as_bytes();
    let mut stack = vec![Ctx::Plain];
    // 二重引用符の中として通した並びがあったか（引用の読み取りが閉じなければ止める）
    let mut passed_as_double = false;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(body) = bodies.iter().find(|b| b.text.start == i && b.resume > i) {
            for (n, next) in runs(&bytes[body.text.clone()], bytes.get(body.text.end).copied()) {
                if body.quoted || n != 2 || special_after_backslash(next) {
                    return true;
                }
            }
            i = body.resume;
            continue;
        }
        let ctx = *stack.last().expect("Plain は外さない");
        let b = bytes[i];
        let next = |k: usize| bytes.get(i + k).copied();
        if b == b'\\' {
            let n = bytes[i..].iter().take_while(|c| **c == b'\\').count();
            let after = next(n);
            if n >= 2 && after != Some(b'"') {
                match ctx {
                    Ctx::Comment => {}
                    Ctx::Double if n == 2 && !special_after_backslash(after) => {
                        passed_as_double = true;
                    }
                    _ => return true,
                }
            }
            // 奇数の並びの最後の 1 つは、次の 1 文字を引用する（単引用符とコメントの中を除く）
            let escapes_next = n % 2 == 1 && !matches!(ctx, Ctx::Single | Ctx::Comment);
            i += n + usize::from(escapes_next && after.is_some());
            continue;
        }
        let word_start = i == 0
            || matches!(
                bytes[i - 1],
                b' ' | b'\t' | b'\n' | b';' | b'&' | b'|' | b'(' | b')'
            );
        match (ctx, b) {
            (Ctx::Plain | Ctx::Subst(_), b'#') if word_start => stack.push(Ctx::Comment),
            (Ctx::Comment, b'\n') => {
                stack.pop();
            }
            (Ctx::Plain | Ctx::Subst(_) | Ctx::Backtick, b'\'') => stack.push(Ctx::Single),
            (Ctx::Single | Ctx::AnsiC, b'\'') => {
                stack.pop();
            }
            (Ctx::Plain | Ctx::Subst(_), b'$') if next(1) == Some(b'\'') => {
                stack.push(Ctx::AnsiC);
                i += 1;
            }
            (Ctx::Plain | Ctx::Subst(_) | Ctx::Double, b'$') if next(1) == Some(b'(') => {
                stack.push(Ctx::Subst(0));
                i += 1;
            }
            (Ctx::Plain | Ctx::Subst(_) | Ctx::Backtick, b'"') => stack.push(Ctx::Double),
            (Ctx::Double, b'"') => {
                stack.pop();
            }
            (Ctx::Backtick, b'`') => {
                stack.pop();
            }
            (Ctx::Plain | Ctx::Subst(_) | Ctx::Double, b'`') => stack.push(Ctx::Backtick),
            (Ctx::Subst(depth), b'(') => {
                *stack.last_mut().expect("Subst がある") = Ctx::Subst(depth + 1)
            }
            (Ctx::Subst(0), b')') => {
                stack.pop();
            }
            (Ctx::Subst(depth), b')') => {
                *stack.last_mut().expect("Subst がある") = Ctx::Subst(depth - 1)
            }
            _ => {}
        }
        i += 1;
    }
    passed_as_double && !matches!(stack.as_slice(), [Ctx::Plain] | [Ctx::Plain, Ctx::Comment])
}

/// text の中の、2 つ以上で `"` の直前でないバックスラッシュの並びの（長さ, 次の文字）。after は text の後ろの文字
fn runs(text: &[u8], after: Option<u8>) -> Vec<(usize, Option<u8>)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] != b'\\' {
            i += 1;
            continue;
        }
        let n = text[i..].iter().take_while(|c| **c == b'\\').count();
        let next = text.get(i + n).copied().or(after);
        if n >= 2 && next != Some(b'"') {
            found.push((n, next));
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
        let shell = Shell::parse(src);
        changes_result(shell.src(), &heredoc_bodies(&shell))
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
            "# a\\\\b\necho a",
            "echo a # it's a\\\\b",
            "echo a",
        ] {
            assert!(!changes(src), "{src}");
        }
    }

    #[test]
    fn an_unclosed_quote_does_not_pass_a_run_read_as_double_quoted() {
        assert!(changes(r#"echo "a\\b"'"#));
        assert!(changes("echo \"a\\\\b"));
    }
}
