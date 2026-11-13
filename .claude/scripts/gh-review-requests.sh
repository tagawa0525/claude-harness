#!/bin/sh
# gh-review-requests.sh — PR の Copilot 宛てレビュー要求の時刻を発生順に出す。中身は claude-hooks script
# gh-review-requests（Rust にして Windows でも動かす）。スキルの文とほかのスクリプトはこのスクリプトの名前とパスで
# 呼ぶので、ファイルは残して呼ぶ
#
# Usage: gh-review-requests.sh <PR の URL>
#
# 出力: review_requested イベントの created_at（ISO 8601）を 1 行ずつ、古い順。
# 要求が無ければ何も出さずに終了コード 0。取得に失敗したら gh の終了コード。
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script gh-review-requests "$@"
