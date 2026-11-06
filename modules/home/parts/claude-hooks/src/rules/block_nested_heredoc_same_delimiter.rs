//! block-nested-heredoc-same-delimiter: ヒアドキュメントの本文に外側と同じ区切りのヒアドキュメントを書き、外側を
//! 途中で終わらせたコマンドを止める
//!
//! bash は外側の本文を、最初の区切りだけの行で終える。本文の中で同じ区切りのヒアドキュメントを開き、その閉じの行を
//! 書くと、外側はその行で終わり、外側の閉じのつもりで書いた行までの残りがシェルのコマンドとして走る。
//! 止めるのは、本文に同じ区切りの開き（`<<D`、`<<'D'`、`<<"D"`、`<<\D`、`<<-D`。行のどこでも）があり、
//! 閉じの行より後のシェルの行（後ろのヒアドキュメントの本文と閉じを除く）に、前後の空白を除くと区切りだけの行が
//! あるとき（外側の閉じのつもりの行が、コマンドとして残る）。後ろのヒアドキュメントは、木でなく原文の行から bash と
//! 同じ順に読む: 外側が途中で終わると、tree-sitter-bash は残りの行を外側と違う所で切り、後ろのヒアドキュメントの
//! 開きを本文の中に入れることがある。
//! 本文の中の開きだけでは止めない: 依頼文や issue の本文の文中の言及（`` `cat <<'EOF' > f` ``）と、内側の閉じを
//! 字下げした入れ子は、外側を途中で終わらせない。エスケープは置かない（止めるときは、行がシェルで走ることが
//! 決まっていて、外側の区切りを変えれば必ず直る）

use std::sync::LazyLock;

use regex::Regex;

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct BlockNestedHeredocSameDelimiter;

impl Rule for BlockNestedHeredocSameDelimiter {
    fn name(&self) -> &'static str {
        "block-nested-heredoc-same-delimiter"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        for span in shell.heredocs() {
            let delim = &span.heredoc.delim;
            // 外側を終わらせた閉じに対応するのは、本文の中の最後の開き
            let Some(inner) = span
                .heredoc
                .body
                .split('\n')
                .rfind(|l| openers(l).iter().any(|(_, d)| d == delim))
            else {
                continue;
            };
            if stray_close(span.rest, delim) {
                return vec![Finding::Deny(deny(delim, span.line.trim(), inner.trim()))];
            }
        }
        Vec::new()
    }
}

/// ヒアドキュメントを開く演算子と、区切りの語（1 が `<<-` の `-`、2 が語）。語はシェルの区切りの文字の前まで
static OPENER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<<(-?)[ \t]*([^\s<>;&|()`]+)").expect("valid regex"));

/// 行の中のヒアドキュメントの開き（`<<-` か、引用符と `\` を外した区切り）を順に。引用符と `\` の外し方は
/// `Shell::heredoc` と同じ（`<<E'OF'` は `EOF`）。語は引用符の中でも空白とシェルの区切りの文字で切るので、それを含む
/// 引用符の区切り（`<<'END MSG'`）は `Shell::heredoc`（`END MSG`）と違って頭だけを読む。here string（`<<<`）は除く
fn openers(line: &str) -> Vec<(bool, String)> {
    OPENER
        .captures_iter(line)
        .filter(|c| !line[..c.get(0).map_or(0, |m| m.start())].ends_with('<'))
        .map(|c| {
            let delim: String = c[2]
                .chars()
                .filter(|ch| !matches!(ch, '\'' | '"' | '\\'))
                .collect();
            (&c[1] == "-", delim)
        })
        .filter(|(_, d)| !d.is_empty())
        .collect()
}

/// `rest` のシェルの行に、前後の空白を除くと `delim` だけの行があるか。開きのある行の後ろは、その区切りの閉じの行
/// までを本文として飛ばす（同じ行に複数あれば順に）
fn stray_close(rest: &str, delim: &str) -> bool {
    let mut lines = rest.split('\n');
    while let Some(line) = lines.next() {
        let opened = openers(line);
        for (dash, close) in &opened {
            for body in lines.by_ref() {
                let body = if *dash {
                    body.trim_start_matches('\t')
                } else {
                    body
                };
                if body == close {
                    break;
                }
            }
        }
        if opened.is_empty() && line.trim() == delim {
            return true;
        }
    }
    false
}

fn deny(delim: &str, outer: &str, inner: &str) -> String {
    format!(
        "ヒアドキュメントの本文に、外側と同じ区切り（{delim}）で開くヒアドキュメントがあります。\n\
         外側: {outer}\n\
         本文の中: {inner}\n\
         bash は外側の本文を、最初の {delim} だけの行（内側の閉じのつもりの行）で終えるので、その後ろの行は本文でなく\
         シェルのコマンドとして走ります（本文のつもりで書いた変数は、外側のシェルでは空のまま展開されます）。\n\
         外側の区切りを、本文に出ない名前にしてください（例: python3 - <<'PYEOF' … PYEOF）。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openers_read_each_quoting_and_the_dash_form() {
        let eof = |dash| vec![(dash, "EOF".to_string())];
        for line in [
            "cat <<EOF",
            "cat <<'EOF' > f",
            "cat << \"EOF\"",
            "cat <<\\EOF",
            "cat <<E'OF'",
            "-m \"$(cat <<'EOF'",
            "x=$(cat <<EOF)",
        ] {
            assert_eq!(openers(line), eof(false), "{line}");
        }
        assert_eq!(openers("cat <<-EOF"), eof(true));
    }

    #[test]
    fn openers_read_the_whole_delimiter_word_and_each_operator_on_the_line() {
        assert_eq!(openers("cat <<123"), vec![(false, "123".to_string())]);
        assert_eq!(
            openers("git commit -F - <<END-MSG"),
            vec![(false, "END-MSG".to_string())]
        );
        assert_eq!(
            openers("cat <<A<<'B'"),
            vec![(false, "A".to_string()), (false, "B".to_string())]
        );
    }

    #[test]
    fn stray_close_skips_the_bodies_of_later_heredocs() {
        assert!(stray_close("echo x\nEOF\n", "EOF"));
        assert!(stray_close("\tEOF \n", "EOF"));
        assert!(!stray_close("cat <<'EOF'\nx\nEOF\n", "EOF"));
        assert!(!stray_close("make -f - <<-MK\n\tEOF\n\tMK\n", "EOF"));
        assert!(!stray_close("cat <<A <<\"B\"\nEOF\nA\nEOF\nB\n", "EOF"));
        assert!(stray_close("cat <<<EOF\nEOF\n", "EOF"));
    }

    #[test]
    fn openers_skip_here_strings_and_keep_longer_words_apart() {
        assert!(openers("cat <<<EOF").is_empty());
        assert!(openers("cat <<< \"$x\"").is_empty());
        for line in ["cat <<EOFX", "cat <<'EOFX'", "x << EOF_2"] {
            assert!(openers(line).iter().all(|(_, d)| d != "EOF"), "{line}");
        }
    }
}
