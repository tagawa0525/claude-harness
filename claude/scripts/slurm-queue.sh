#!/bin/sh
# slurm-queue.sh — Slurm の待ち行列を、slurm-run.sh がコメントに入れた区分と目的つきで、走る順に並べる。中身は
# claude-hooks script slurm-queue（並べ方を、起動せずに Rust の単体のテストで確かめるため）。スキルの文と slurm-run.sh は
# このスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: slurm-queue.sh                           # 一覧
#        slurm-queue.sh --order [--first <番号>]  # main を区分の順に並べる番号の列（slurm-run.sh が投げた後に使う）
#
# 一覧の行、NOTE:、ORDER:、NEXT: の形と並べ方は crates/claude-hooks/src/script/slurm_queue.rs の先頭に書く。
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script slurm-queue "$@"
