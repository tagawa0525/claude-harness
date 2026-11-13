#!/bin/sh
# base-moved.sh — 比較元が分岐点から進み、ブランチと同じパスを触っていないかを知らせる。中身は claude-hooks script
# base-moved（Rust にして Windows でも動かす。fetch するかの決め方と WARN もそこに書く）。スキルの文はこのスクリプトの
# 名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: base-moved.sh [branch] [--base BASE]
#   branch  省略時は今のブランチ
#   BASE    省略時は origin の既定ブランチ（origin/HEAD → origin/main → origin/master）。
#           積み重なったブランチは土台を origin/<土台のブランチ> で渡す（fetch が更新するのは
#           origin/* だけで、ローカルのブランチは古いまま比べることになる）。リモートの無いリポジトリでは
#           --base main を渡す（GitHub の origin が無く --base も無ければ何もしない）
#
# 出力（BASE が分岐点から進み、進んだ変更とブランチの変更が同じパスを触るときだけ。それ以外は何も出さない）:
#   BASE_MOVED: <BASE> が分岐点から <N> commits 進んでいる（先頭 <hash>、分岐点 <hash>）
#   COMMON: <両方が触ったパス>   パスごとに 1 行
# WARN が出たときは比較が済んでいない（出力が空でも「進みなし」ではない）
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script base-moved "$@"
