#!/bin/sh
# branch-topics.sh — ブランチが含むトピックを要約する（1 ブランチ 1 トピックの確認用）。中身は claude-hooks script
# branch-topics（Rust にして Windows でも動かす。ADR と issue の数え方もそこに書く）。スキルの文はこのスクリプトの名前と
# パスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: branch-topics.sh [base [head]]
#   base を省くと既定ブランチ（origin/HEAD → origin/main → origin/master → main → master の最初にあるもの）と比べる。
#   変更は merge-base から head まで（base...HEAD）を見る。head を渡すと HEAD の代わりにそのコミットを要約する
#   （チェックアウトしていない PR のブランチを見るとき。例: origin/main origin/feat/x）
#
# 出力（行がないものは省く）: BASE、HEAD、COMMITS、TYPES、ADR_NEW、ADR_STATUS、ISSUE_NEW、ISSUE_STATUS、DIRS
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script branch-topics "$@"
