#!/bin/sh
# gh-pr-review: レビュースレッドを resolve するスクリプト。中身は claude-hooks script resolve-thread（Rust にして
# Windows でも jq なしで動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して exec する
#
# Usage: resolve-thread.sh <PR の URL> <comment_id> [--allow-human]
#
#   comment_id: スレッド内の任意のレビューコメントの数値 ID
#               （get-review-comments.sh の id / URL の #discussion_r{id}）
#
# 人間が起こしたスレッドは既定で resolve せず、--allow-human はユーザーが明示的に求めたときだけ付ける
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script resolve-thread "$@"
