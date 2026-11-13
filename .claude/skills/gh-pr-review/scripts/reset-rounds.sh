#!/bin/sh
# gh-pr-review: レビューの周回を 0 から数え直す印のコメントを PR に付ける。中身は claude-hooks script reset-rounds
# （Rust にして Windows でも動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して exec する
#
# Usage: reset-rounds.sh <PR の URL> <理由>
#
# Windows の Git Bash は、ネイティブのコマンドに渡す / で始まる引数（"/gh-pr-review の後の要望" のような理由）を
# Windows のパスに書き換えるので、書き換えを止めて渡す
export MSYS_NO_PATHCONV=1
exec "${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}" script reset-rounds "$@"
