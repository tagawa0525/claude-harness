#!/bin/sh
# gh-pr-review: PR情報取得スクリプト。中身は claude-hooks script get-pr-info（Rust にして Windows でも jq なしで
# 動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して exec する
#
# Usage: get-pr-info.sh <PR の URL | コメントの URL>
#
# URL: https://github.com/{owner}/{repo}/pull/{n}[#discussion_r{comment_id}]
#      comment_id（#discussion_r でなければ null）を PR 情報に添える
# Output: JSON形式のPR情報
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script get-pr-info "$@"
