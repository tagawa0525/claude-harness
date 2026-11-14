#!/bin/sh
# kept-sentences.sh — 規則の文書を縮めたとき、今の版の文のうち元の版に無いものを出す。中身は
# claude-hooks script kept-sentences（Rust にして Windows でも動かす。ADR-0088）
#
# Usage: kept-sentences.sh --old <元の版>... --new <今の版>...
#   --old  縮める前の版（複数なら 1 つにつないで比べる）。git の版は `<(git show <base>:<path>)` で渡す
#   --new  縮めた後の版（文を別のファイルに移したときは、移した先も並べる）
#
# 出力（元に無い文ごと）: CHANGED: <今の版のファイル>:<文の始まる行>: <文>
# 終了コード: 0 どの文も元の版にある / 1 元に無い文がある / 2 使い方の誤り、ファイルが読めない
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script kept-sentences "$@"
