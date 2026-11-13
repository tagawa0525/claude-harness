#!/bin/sh
# git-merge-state.sh — /git-merge の事前確認（対象の検証と、マージの前に要る状態の収集）。中身は claude-hooks script
# git-merge-state（Rust にして Windows でも動かす。止まる条件と各行の意味もそこに書く）。スキルの文はこのスクリプトの
# 名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: git-merge-state.sh [branch]
#   branch を省くと今のブランチを対象にする。対象が無い・既定ブランチ自身、GitHub リモートがある、未コミットの変更が
#   あるときは ERROR を出して exit 1（マージに進まない）
#
# 出力: TARGET、DEFAULT、COMMITS、BEHIND（既定ブランチが無ければ ROOT_COMMITS）、STACKED_ON、TARGET_WORKTREE、
#   DEFAULT_WORKTREE、PROPOSED_ADRS（feat / fix のコミットが Refs: で挙げた、対象の先端で proposed の ADR。実装を
#   マージするブランチは、同じブランチで accepted にする）
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script git-merge-state "$@"
