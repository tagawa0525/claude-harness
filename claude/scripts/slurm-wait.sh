#!/bin/sh
# slurm-wait.sh — 既にある Slurm のジョブの終わりを番号から待ち、slurm-run.sh の終わりと同じ行を出す。/delegate の担当が
# 待ちの線（55 分）を超えて終えた後に、親が担当の投げたジョブを引き継いで待つ。番号を複数受け、すべてが終わってから
# 1 度だけ終わる（親の background の終わりの通知を 1 回にする）。中身は claude-hooks script slurm-wait
# （終わりの記録の読み方と行の組み立ては slurm-run と同じ関数）。hook の guard-sbatch は background でだけ通し、
# guard-delegate-wait は担当の呼び出しを止める
#
# Usage: slurm-wait.sh <ジョブの番号>...
#
# 出力（JOB: と LOG: は読めたジョブから順に、その後の行はジョブごとに番号の順に）:
#   JOB: <番号>
#   LOG: <このホストから読めるログのパス>
#   （ログの末尾 20 行。TIME: の行は、slurm-run.sh がまだ書いていなければログへ書き足すので、その行も含む。区分、参照、
#    目的、-c の印は、slurm-run.sh がジョブのコメントに入れたものを読む。終わってから 5 分を過ぎて制御のデーモンが
#    忘れたジョブは、コメントを読めないので - になる）
#   NEXT: （TIME 行に印があるときだけ）その時間を記録に使わないことと、測り直し方
#   RESULT: job=<番号> name=<名前> exit=<終了コード> state=<State>
#   EXIT: <終了コード>（COMPLETED 以外で終わったときは、後ろに (State=<State>) を付ける）
# ジョブごとの終了コードは slurm-run.sh と同じく、ジョブの終了コード（シグナルで終われば 1）で、COMPLETED でなく 0 なら 1。
# 制御のデーモンにも会計の記録にも無いジョブは 1。全体の終了コードは、番号の順で最初の 0 でないもの。番号の誤りは 1。
# 中断（INT、TERM、HUP）されたら 130。
# 終わりは制御のデーモン（scontrol show job）を SLURM_WAIT_INTERVAL 秒（既定 30）ごとに読んで待つ（読めなければ読み直す）。

. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script slurm-wait "$@"
