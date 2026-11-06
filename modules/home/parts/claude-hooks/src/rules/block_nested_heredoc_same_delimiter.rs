//! block-nested-heredoc-same-delimiter: ヒアドキュメントの本文に外側と同じ区切りのヒアドキュメントを書き、外側を
//! 途中で終わらせたコマンドを止める
//!
//! bash は外側の本文を、最初の区切りだけの行で終える。本文の中で同じ区切りのヒアドキュメントを開き、その閉じの行を
//! 書くと、外側はその行で終わり、外側の閉じのつもりで書いた行までの残りがシェルのコマンドとして走る。
//! 止めるのは、本文に同じ区切りの開き（`<<D`、`<<'D'`、`<<"D"`、`<<\D`、`<<-D`。行のどこでも）があり、
//! 閉じの行より後に、区切りだけの行が同じ区切りの開きより多いとき（外側の閉じのつもりの行が余る）。
//! 本文の中の開きだけでは止めない: 依頼文や issue の本文の文中の言及（`` `cat <<'EOF' > f` ``）と、内側の閉じを
//! 字下げした入れ子は、外側を途中で終わらせない。エスケープは置かない（止めるときは、行がシェルで走ることが
//! 決まっていて、外側の区切りを変えれば必ず直る）

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
            let opener = opener(delim);
            let Some(inner) = span.heredoc.body.lines().find(|l| opener.is_match(l)) else {
                continue;
            };
            let opens: usize = span.rest.lines().map(|l| opener.find_iter(l).count()).sum();
            let closes = span
                .rest
                .lines()
                .filter(|l| l.trim_start_matches('\t') == delim)
                .count();
            if closes > opens {
                return vec![Finding::Deny(deny(delim, span.line.trim(), inner.trim()))];
            }
        }
        Vec::new()
    }
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
    fn opener_skips_here_strings_and_longer_words() {
        let re = opener("EOF");
        for line in ["cat <<<EOF", "cat <<EOFX", "cat <<'EOFX'", "x << EOF_2"] {
            assert!(!re.is_match(line), "{line}");
        }
    }
}
