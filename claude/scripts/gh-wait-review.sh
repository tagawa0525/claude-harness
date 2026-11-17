#!/bin/sh
# gh-wait-review.sh — PR の自動レビューの到着を、間隔を延ばしながら待つ。中身は claude-hooks script gh-wait-review
# （Rust にして Windows でも jq なしで動かす。基準の時刻の決め方と終了コードもそこに書く）。スキルの文と
# request-rereview.sh、PreToolUse のルール（フォアグラウンドで待たせない）はこのスクリプトの名前とパスで呼び、見分ける
# ので、ファイルは残して呼ぶ
#
# 使い方: gh-wait-review.sh <PR の URL> [--since <ISO 8601 時刻>]
#
# 終了コード: 0 = レビュー到着、1 = タイムアウト（約10分）、3 = 対象PRが見つからない、4 = gh が利用できない、
#   5 = 引数エラー、6 = レビュー要求がない（待っても来ない）か取得できない、7 = レビューの実行が失敗している
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script gh-wait-review "$@"
