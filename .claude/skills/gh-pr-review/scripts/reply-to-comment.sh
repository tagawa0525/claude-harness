#!/bin/sh
# gh-pr-review: コメント返信スクリプト。中身は claude-hooks script reply-to-comment（Rust にして Windows でも jq なしで
# 動かす）。スキルの文と PreToolUse のルールはこのスクリプトの名前とパスで呼び、見分けるので、ファイルは残して exec する
#
# Usage: reply-to-comment.sh <PR の URL> <comment_id> <body>
#
# Output: 作成された返信の情報
# Windows の Git Bash は、ネイティブのコマンドに渡す / で始まる引数（"/gh-pr-review で直した" のような本文）を Windows の
# パスに書き換えるので、書き換えを止めて渡す
export MSYS_NO_PATHCONV=1
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script reply-to-comment "$@"
