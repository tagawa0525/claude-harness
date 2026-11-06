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
            // 開きの無い本文（ほとんど）で、区切りごとの正規表現を作らない
            if !span.heredoc.body.contains("<<") {
                continue;
            }
            let opener = opener(delim);
            // 外側を終わらせた閉じに対応するのは、本文の中の最後の開き
            let Some(inner) = span.heredoc.body.split('\n').rfind(|l| opener.is_match(l)) else {
                continue;
            };
            if stray_close(span.rest, delim) {
                return vec![Finding::Deny(deny(delim, span.line.trim(), inner.trim()))];
            }
        }
        Vec::new()
    }
}

/// どの区切りでもヒアドキュメントを開く演算子（1 が `<<-` の `-`、区切りは 2〜4 のどれか）。here string（`<<<`）は除く
static ANY_OPENER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[^<])<<(-?)[ \t]*(?:'([^']+)'|"([^"]+)"|\\?([A-Za-z_][A-Za-z0-9_]*))"#)
        .expect("valid regex")
});

/// `rest` のシェルの行に、前後の空白を除くと `delim` だけの行があるか。開きのある行の後ろは、その区切りの閉じの行
/// までを本文として飛ばす（同じ行に複数あれば順に）
fn stray_close(rest: &str, delim: &str) -> bool {
    let mut lines = rest.split('\n');
    while let Some(line) = lines.next() {
        let mut opened = false;
        for caps in ANY_OPENER.captures_iter(line) {
            opened = true;
            let dash = &caps[1] == "-";
            let close = caps
                .get(2)
                .or(caps.get(3))
                .or(caps.get(4))
                .map_or("", |m| m.as_str());
            for body in lines.by_ref() {
                let body = if dash {
                    body.trim_start_matches('\t')
                } else {
                    body
                };
                if body == close {
                    break;
                }
            }
        }
        if !opened && line.trim() == delim {
            return true;
        }
    }
    false
}

/// 区切り `delim` でヒアドキュメントを開く演算子。here string（`<<<`）と、区切りが続く語の頭であるもの（`<<EOFX`）は除く
fn opener(delim: &str) -> Regex {
    let d = regex::escape(delim);
    Regex::new(&format!(
        r#"(?:^|[^<])<<-?[ \t]*(?:'{d}'|"{d}"|\\?{d})(?:$|[^A-Za-z0-9_])"#
    ))
    .expect("valid regex")
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
    fn opener_matches_each_quoting_and_the_dash_form() {
        let re = opener("EOF");
        for line in [
            "cat <<EOF",
            "cat <<'EOF' > f",
            "cat << \"EOF\"",
            "cat <<\\EOF",
            "cat <<-EOF",
            "-m \"$(cat <<'EOF'",
        ] {
            assert!(re.is_match(line), "{line}");
        }
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
    fn opener_skips_here_strings_and_longer_words() {
        let re = opener("EOF");
        for line in ["cat <<<EOF", "cat <<EOFX", "cat <<'EOFX'", "x << EOF_2"] {
            assert!(!re.is_match(line), "{line}");
        }
    }
}
