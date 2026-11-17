#!/bin/sh
# resume-state.sh — 再開時に、開いている PR・worktree・Slurm のジョブ・docs/issues の issue を 1 つの出力に集める。
# 中身は claude-hooks script resume-state（Rust にして Windows でも動かす。出力の各行の意味もそこに書く）。スキルの
# `!` 行はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: resume-state.sh        （対象のリポジトリの中で実行する）
#
# 出力: `== worktrees`、`== prs`、`== slurm`、`== issues` の節（各行と印の意味は claude-hooks の resume_state.rs）
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script resume-state "$@"
