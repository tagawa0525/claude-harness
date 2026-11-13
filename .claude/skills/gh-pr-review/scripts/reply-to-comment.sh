#!/bin/sh
# gh-pr-review: コメント返信スクリプト。中身は claude-hooks script reply-to-comment（ADR-0087）
#
# Usage: reply-to-comment.sh <PR の URL> <comment_id> <body>
#
# Output: 作成された返信の情報
exec "${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}" script reply-to-comment "$@"
