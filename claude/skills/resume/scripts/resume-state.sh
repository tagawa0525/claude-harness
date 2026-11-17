#!/bin/sh
# resume-state.sh — 再開時に、開いている PR・worktree・Slurm のジョブ・docs/issues の issue を 1 つの出力に集める。
# 中身は claude-hooks script resume-state（Rust にして Windows でも動かす。出力の各行の意味もそこに書く）。スキルの
# `!` 行はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: resume-state.sh        （対象のリポジトリの中で実行する）
#
# 出力（節ごとに `== <名前>` の見出し。集められないものは `(なし)` か理由の 1 行。失敗では止めない）:
#   == worktrees   <パス> <ブランチ> [dirty] [session=<Claude-Session>]（先端のコミットの trailer、無ければ省く）
#   == prs         gh pr list の開いている PR（GitHub の URL を持つ remote と gh があるときだけ）
#   == slurm       squeue の自分のジョブ
#   == issues      docs/issues の status: open の issue（blocked-by があれば添える）
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script resume-state "$@"
