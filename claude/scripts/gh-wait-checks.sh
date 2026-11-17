#!/bin/sh
# gh-wait-checks.sh — PR の head の check（CI）が登録され、全部が終わるまで、間隔を延ばしながら待つ。中身は
# claude-hooks script gh-wait-checks（待ち方と終了コードもそこに書く）。gh pr checks --watch は、push の直後に check が
# まだ登録されていないと、すぐ終わってしまう。PreToolUse のルール（フォアグラウンドで待たせない）はこのスクリプトの
# 名前で見分けるので、ファイルは残して呼ぶ
#
# 使い方: gh-wait-checks.sh <PR の URL>
#
# 出力: HEAD: <SHA>、CHECKS: passed | failed | none、FAILED: <名前> (<結論>)
# 終了コード: 0 = 全部が通った（か check が無い）、1 = タイムアウト（約 20 分）、2 = 失敗した check がある、
#   3 = 対象 PR が見つからない、4 = gh が利用できない、5 = 引数エラー
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script gh-wait-checks "$@"
