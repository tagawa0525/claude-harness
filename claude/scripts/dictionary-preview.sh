#!/bin/sh
# dictionary-preview.sh — セッションの記録のアシスタントの発言とメモリに ja-terms の辞書をかけ、辞書がどの語をどう
# 置き換えるかを種類ごとに件数と例つきで出す。中身は claude-hooks script dictionary-preview（Rust にして Windows でも
# 動かす。発言の抜き出し方と出力の詳しい形もそこに書く）
#
# Usage: dictionary-preview.sh --dictionary <辞書> [--base <辞書>] [--examples <N>] [--projects <DIR>]
#   --dictionary  かける辞書（prh.yml）
#   --base        比較元の辞書。渡すと、--dictionary だけにある指摘（NEW:）と --base だけにある指摘（GONE:）を出す
#   --examples    種類ごとに出す例の数（既定は 3）
#   --projects    記録とメモリのディレクトリ（既定は ~/.claude/projects）
#
# 使いどころ: 辞書の規則を足す・変える PR で、置き換えた後の文を読み、活用の崩れや別の語の一部への一致を見つける
# （specs は書いた人が思いついた例しか見ない）。比較元は、変える前の辞書を書き出して渡す。例:
#   git show origin/main:modules/home/parts/ja-terms/prh.yml > "$TMPDIR/base.yml"
#   dictionary-preview.sh --dictionary modules/home/parts/ja-terms/prh.yml --base "$TMPDIR/base.yml"
#
# 出力:
#   SOURCES: transcripts=<記録の数> utterances=<発言の行の数> memory=<メモリのファイルの数>
#   HIT: <件数> <誤り> => <正しい語>     発言への指摘（--base が無いとき）。続く行に、字下げした例（誤りの語を【】で囲む）
#   NEW: / GONE:                         --base があるときの差（形は HIT: と同じ）
#   MEMORY: <project>/memory/<file>:<行> <誤り> => <正しい語>
#                                        メモリへの --dictionary の指摘（--base にもある指摘も出す。メモリは直す対象）
# 指摘があっても exit 0（読んで判断するのは呼んだ側）。引数の誤り、記録のディレクトリを読めない、辞書の検査を実行
# できないときは、ERROR: か使い方を標準エラーに出して exit 1。
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script dictionary-preview "$@"
