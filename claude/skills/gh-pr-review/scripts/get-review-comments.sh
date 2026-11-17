#!/bin/sh
# gh-pr-review: レビューコメント取得スクリプト。中身は claude-hooks script get-review-comments（Rust にして Windows
# でも jq なしで動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して exec する
#
# Usage: get-review-comments.sh <PR の URL> [--unresolved]
#
# Output: JSON 配列。1 要素 = 1 コメント（スレッド内の返信も含む、作成順）
#   id, node_id, path, line, body, user, user_type（Bot / User。resolve の可否判断に使う）,
#   created_at, in_reply_to_id, html_url,
#   thread_id（resolve-thread.sh の対象）, is_resolved, is_outdated
# --unresolved は resolve していないスレッドに限る
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script get-review-comments "$@"
