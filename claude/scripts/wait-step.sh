#!/bin/sh
# wait-step.sh — /delegate の担当が、ターンを終えずに待つ 1 ステップ。中身は claude-hooks script wait-step（Rust にして
# Windows でも coreutils なしで動かす）
#
# Usage: wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...]
#        wait-step.sh --end <自分の worktree のパス>
#
# 出力と終了コード:
#   DONE: <ファイル>                           0  渡した background の出力のどれかが終わった
#   WAIT: 待つだけで <経過> 分                 0  1 ステップ（約 4 分 30 秒）の時間がたった（まだ待つ。失敗ではないので
#                                                 0 にし、DONE とは行で見分ける）
#   END: 待つだけで <経過> 分を超えた…         3  待つだけ（1 ステップの呼び直し）が線（55 分）を超えて続いた。担当は
#                                                 待ちをやめ、投げたジョブは止めずに、残りの作業を報告に書いて終える
#   （待つだけの時間は、DONE、--end、前の呼び出しに無かった出力のファイル、前の呼び出しから 1 ステップを超える空きで
#    数え直す）
#   使い方の誤り、渡した出力のファイルが無い、状態を書けないときは、理由を出して 1
# DONE、WAIT、END の行の後に、次の行動を NEXT: の行で出す
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script wait-step "$@"
