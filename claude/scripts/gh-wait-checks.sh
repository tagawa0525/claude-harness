#!/bin/sh
# gh-wait-checks.sh — PR の head の check（CI）が登録され、全部が終わるまで、間隔を延ばしながら待つ。中身は
# claude-hooks script gh-wait-checks（待ち方と終了コードもそこに書く）。gh pr checks --watch は、push の直後に check が
# まだ登録されていないと、すぐ終わってしまう。PreToolUse のルール（フォアグラウンドで待たせない）はこのスクリプトの
# 名前で見分けるので、ファイルは残して呼ぶ
#
# 使い方: gh-wait-checks.sh <PR の URL> [<push したコミットの SHA>]
#   SHA を渡すと、PR の head がそのコミットになるまで待ってから check を見る（push の直後は、PR の head がまだ前の
#   コミットのことがあり、渡さないと前の head の結果を返しうる）
#
# 出力: HEAD: <SHA>、CHECKS: passed | failed | none、FAILED: <名前> (<結論>)
# 終了コード: 0 = 全部が通った（か check が無い）、1 = タイムアウト（約 20 分）、3 = 対象 PR が見つからない、
#   4 = gh が利用できないか check の状態を取れない、5 = 引数エラー、6 = 失敗した check がある
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script gh-wait-checks "$@"
