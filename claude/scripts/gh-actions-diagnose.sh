#!/bin/sh
# gh-actions-diagnose.sh — CI の最新結果を調べ、失敗の原因を分類する。中身は claude-hooks script gh-actions-diagnose
# （Rust にして Windows でも jq なしで動かす）。スキルの文と拒否の文はこのスクリプトの名前とパスで呼ぶので、ファイルは
# 残して呼ぶ
#
# Usage: gh-actions-diagnose.sh [PR の URL | PR番号 | ブランチ名]
#   省略時は現在のブランチ。PR なら head ブランチと head SHA を解決する
#   PR の URL なら、gh を URL のリポジトリに向け、NEXT の gh の手順に -R を付ける。
#   PR 番号・ブランチ名・省略時は cwd のリポジトリで読む
#
# 見るもの: GitHub Actions の run（失敗ならジョブ・ステップ・ログから原因を分類）と、head SHA の check-run のうち
#   GitHub Actions 以外の App（外部 CI）と commit status（gh run rerun では再実行できない）
#
# 出力: BRANCH、PR、RUNS、RUN、STATUS、ATTEMPT、FAILED_JOB、FAILED_STEP、--- errors ---、EXTERNAL_CHECKS、
#   EXTERNAL_CHECK、CAUSE、NEXT
#   CAUSE: NONE（run が無いか成功）、IN_PROGRESS（実行中）、PERMISSION（権限不足。ワークフローの permissions: を最小限で
#   宣言し、default_workflow_permissions の緩和で回避しない）、TRANSIENT_API（GitHub API の一時障害）、COPILOT_INTERNAL
#   （Copilot の内部エラー）、EXTERNAL（外部 CI の失敗。details_url を見る）、CODE（ビルド・テスト・lint の失敗）
# 再実行の提案は attempt 1 のときだけ（再実行済みで同じ失敗なら報告に切り替える）
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script gh-actions-diagnose "$@"
