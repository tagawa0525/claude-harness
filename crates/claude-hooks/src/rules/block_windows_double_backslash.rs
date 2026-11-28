//! block-windows-double-backslash: Windows で、2 つ続きのバックスラッシュ（`\\`）を含む Bash のコマンドを止める
//!
//! Windows の Bash ツールは、コマンドの文字列の `\\` を、bash に渡す前に `\` 1 つにする。hook には書いたとおりの
//! `\\` が届き、bash には 1 つで届くので、bash の引用の規則では防げない: 引用符つきのヒアドキュメント（`<<'EOF'`）、
//! 引用符なしのヒアドキュメント、`python -c`、単引用符の引数のどれでも 1 つになる。Windows のパスを含む Python の
//! 文字列（`'C:\\Users'` が `'C:\Users'` になり unicodeescape で落ちる）や、ファイルに書いた本文が崩れる。
//! 1 つだけのバックスラッシュは書いたとおりに届くので通す（止める範囲をヒアドキュメントと `python -c` の中に
//! 狭めないのは、どの書き方でも同じく 1 つになるため）。Windows でない機械では止めない（この変換は Windows の
//! Bash ツールのもの）。エスケープは置かない（止めるときは、書いたとおりに届かないことが決まっている）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct BlockWindowsDoubleBackslash;

impl Rule for BlockWindowsDoubleBackslash {
    fn name(&self) -> &'static str {
        "block-windows-double-backslash"
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        if cfg!(windows) && collapses(&input.command) {
            vec![Finding::Deny(DENY.to_string())]
        } else {
            Vec::new()
        }
    }
}

/// Windows の Bash ツールが、書いたものと違う文字列にして bash に渡すか
fn collapses(command: &str) -> bool {
    command.contains(r"\\")
}

const DENY: &str = r"コマンドに 2 つ続きのバックスラッシュ（\\）があります。Windows の Bash ツールは、コマンドの文字列の \\ を bash に渡す前に \ 1 つにします（引用符つきのヒアドキュメント <<'EOF'、引用符なしのヒアドキュメント、python -c、単引用符の引数のどれでも）。書いたとおりに届かず、Windows のパスを含む Python の文字列（'C:\\Users' が 'C:\Users' になり unicodeescape で落ちる）や、ファイルに書いた本文が崩れるので止めます。
代わりに:
- バックスラッシュを含むスクリプトやメッセージは、Write でファイルに書いてから、ファイルで渡す（python <ファイル>、git commit -F <ファイル>。Write はバックスラッシュを書いたとおりに残します）
- Windows のパスは / で区切る（C:/Users/...。Python も Git Bash も受けます）
- 正規表現などでバックスラッシュが 1 つ要るだけなら、1 つで書く（1 つは書いたとおりに届きます）";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_two_consecutive_backslashes_collapse() {
        assert!(collapses(r"echo 'a\\b'"));
        assert!(collapses("cat <<'EOF'\nC:\\\\Users\nEOF"));
        assert!(collapses(r"echo 'a\\\b'"));
        assert!(!collapses(r"echo 'a\b'"));
        assert!(!collapses(r"grep '\.' f"));
        assert!(!collapses("echo a"));
    }
}
