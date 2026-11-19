#!/bin/sh
# wait-step.sh — /delegate の担当が、ターンを終えずに待つ 1 ステップ。中身は claude-hooks script wait-step（Rust にして
# Windows でも coreutils なしで動かす）
#
# Usage: wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...]
#        wait-step.sh --end <自分の worktree のパス>
#
# 出力と終了コード:
#   DONE: <ファイル>                           0  渡した background の出力のどれかが終わった
#   WAIT: 待ち始めから <経過> 分               2  1 ステップ（約 4 分 30 秒）の時間がたった
#   END: 待ち始めから <経過> 分を超えた…       3  待ち始めから線（55 分）を超えた。担当は待ちをやめ、投げたジョブは
#                                                 止めずに、残りの作業を報告に書いて終える
#   使い方の誤り、渡した出力のファイルが無い、状態を書けないときは、理由を出して 1
# DONE、WAIT、END の行の後に、次の行動を NEXT: の行で出す
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script wait-step "$@"
