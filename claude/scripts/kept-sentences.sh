#!/bin/sh
# kept-sentences.sh — 規則の文書を縮めたとき、今の版の文のうち元の版に無いものを出す。中身は
# claude-hooks script kept-sentences（Rust にして Windows でも Python なしで動かす）
#
# Usage: kept-sentences.sh --old <元の版>... --new <今の版>...
#        kept-sentences.sh --base <base> <パス>...
#   --old  縮める前の版（複数なら、各ファイルを別々に文に切って、文の集合をまとめて比べる）
#   --new  縮めた後の版（文を別のファイルに移したときは、移した先も並べる）
#   --base 各パスの <base> の版を元の版に、作業ツリーの版を今の版にする。文を移した先の新しいファイルと、移した元の
#          消したファイルも並べる（<base> に無いものは今の版だけ、作業ツリーに無いものは元の版だけに数える）
#
# 出力（元に無い文ごと）: CHANGED: <今の版のファイル>:<文の始まる行>: <文>
# 終了コード: 0 どの文も元の版にある / 1 元に無い文がある / 2 使い方の誤り、ファイルが読めない
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script kept-sentences "$@"
