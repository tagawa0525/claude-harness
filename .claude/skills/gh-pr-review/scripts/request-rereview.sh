#!/bin/sh
# gh-pr-review: Copilot に再レビューを依頼し、その応答を待つスクリプト。中身は claude-hooks script request-rereview
# （Rust にして Windows でも jq なしで動かす。手順と終了コードもそこに書く）。スキルの文と PreToolUse のルール
# （フォアグラウンドで待たせない）はこのスクリプトの名前とパスで呼び、見分けるので、ファイルは残して呼ぶ
#
# Usage: request-rereview.sh <PR の URL>
#
# Copilot のレビューを受けたいときだけ使う（任意。head のローカルレビューの記録があればマージできる）。
# 約10分待つため、Bash ツールでは run_in_background=true で実行すること
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script request-rereview "$@"
