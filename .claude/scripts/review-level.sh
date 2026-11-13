#!/bin/sh
# review-level.sh — ローカルの /code-review に渡すレベルと対象を決める。中身は claude-hooks script review-level
# （Rust にして Windows でも動かす。「文書」の決め方と比較元の決め方もそこに書く）。スキルの文はこのスクリプトの名前と
# パスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: review-level.sh [branch] [--base BASE]
#
# 出力: BASE、TARGET、WORKTREE（TARGET をチェックアウトしている worktree のパスか none。/code-review の対象に渡す）、
#   FILES、LEVEL（medium | high）。差分が空、または refs を解決できないときは exit 1
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script review-level "$@"
