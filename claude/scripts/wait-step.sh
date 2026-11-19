#!/bin/sh
# wait-step.sh — /delegate の担当が、ターンを終えずに待つ 1 ステップ。中身は claude-hooks script wait-step（Rust にして
# Windows でも coreutils なしで動かす）
#
# Usage: wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...]
#        wait-step.sh --end <自分の worktree のパス>
#
# 出力と終了コード:
#   DONE: <ファイル>                           0  渡した background の出力のどれかが終わった
#   WAIT: 途中の報告から <経過> 分             2  1 ステップ（約 4 分 30 秒）の時間がたった
#   REPORT: 途中の報告から <経過> 分を超えた…  3  途中の報告から 55 分を超えた（数え直した）
#   使い方の誤り、渡した出力のファイルが無い、状態を書けないときは、理由を出して 1
# DONE、WAIT、REPORT の行の後に、次の行動を NEXT: の行で出す
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script wait-step "$@"
