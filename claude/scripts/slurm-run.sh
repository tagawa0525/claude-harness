#!/bin/sh
# slurm-run.sh — r995 の Slurm に重い計算を 1 本投げ、終わるまで待つ。中身は claude-hooks script slurm-run（判定を、起動せずに
# Rust の単体のテストで確かめるため）。スキルの文、hook の guard-sbatch、設定の許可はこのスクリプトの名前とパスで呼び、見分けるので、
# ファイルは残して呼ぶ
#
# Usage: slurm-run.sh --class <区分> --ref <参照> --purpose <目的> [--top] [--timed | -c <コア数>] -t <見積もり> <name> <command>
#
# --class、--ref、--purpose は必須で、待ち行列でジョブの順を目的から確かめるために、ジョブのコメントの先頭に
# 「<区分> | <参照> | <目的> | <連絡先>」の形で入れ（slurm-queue.sh が読む）、TIME 行にも出す。
#   --class: timed（時間を記録する測定。--timed を付けなくてもパーティション timed に投げる）、record（記録に載せる本番）、
#            decision（決定に要る確認）、trial（検討）。この順に優先する
#   --ref: きっかけの issue か ADR の番号（#NNNN、ADR-NNNN。複数はカンマで区切る）。無いときは「- <理由>」
#   --purpose: 結果が何を決めるかの 1 文。改行と | を含めない（コメントの区切りに使う）
# 既定では、ほかの計算と重ならないよう排他で、既定のパーティション main に投げる（sbatch --exclusive --wait --export=ALL）。
# main のジョブは --timed のジョブに止められ（SUSPEND）、その終わりに続きから再開する。いつ止められてもよい条件で
# 走るので、時間は記録に使わず、TIME 行に「割り込まれうる」印を常に付ける（止められなかったときも）。
# <name> はジョブの名前（squeue で見分ける）とログのファイル名に使う。例: xlc-2G、openmc-2G
# -t: かかる時間の見積もり（sbatch --time の形。分か H:MM:SS）。待ち行列の見込み（squeue --start）に
#     使う。超えても止めない（slurm.conf の OverTimeLimit=UNLIMITED）
# -c: 排他にせず、<コア数> の CPU（Slurm の CPUs。ハードウェアスレッドで数え、割り当ては物理コア単位に
#     切り上がる）だけを取って実行する（--cpus-per-task）。時間を記録しない長い計算のためで、ほかのジョブと
#     資源を取り合うので、TIME 行に「時間は当てにならない」印を付ける。排他の main のジョブはノードにジョブが
#     走っている間は始まらないので、走っている間は待たせる（--timed のジョブは待たず、走っているジョブを止める）
# --top: 同じ区分の待っているジョブより先に実行する（main。上の区分のジョブは追い越さない）。timed では待っている
#     ジョブすべてより先に実行する。走っているジョブは止めない
# main に投げたときは、待っているジョブを区分の順に並べ直す（slurm-queue.sh --order と同じ列を scontrol top に渡す）
# --timed: 時間を記録するジョブ（と数分の急ぐ計算）。排他で、割り込まれないパーティション timed に投げる。待っている
#     main のジョブより先に走り、走っている main のジョブをすべて止めて実行し、自分は止められない。timed のジョブどうしは
#     止め合わず、投げた順に走る。止めたジョブはメモリに残る（Slurm はメモリを数えない）ので、走っているジョブとの合計が
#     搭載量に収まる計算に限る。-c と組めない（排他でない時間は記録に使えない）
# 連絡先として、git のブランチ、投げた場所、投げた時点の HEAD のコミット、Claude Code のセッションの ID をジョブの
# コメントに書く（squeue の %k。sacct では SubmitLine の --comment）。サブエージェントは親と同じセッションなので、
# ブランチで見分ける。コミットは「commit=<短い hash>」で、コミットしていない変更（git が無視しない、まだ追加して
# いないファイルを含む）があれば「commit=<短い hash>-dirty」（作業中の版で、そのコミットの中身とは違う）。git の外とコミットの無いリポジトリでは付けない（git の中で調べられなければ、警告して付けずに投げる）。
# 記録に書く結果の出した版を、ブランチが消えた後もジョブから引くため。
#
# ログは r995 の ~/github/slurm-logs/<name>-<job>.out。ジョブは投げたときのパスで r995 の上で走るので、
# r995 以外のホストでは /mnt/r995/github（r995 の ~/github の NFS。テストは SLURM_RUN_NFS_ENTRY で置き換える）の下で投げる。
#
# 出力:
#   JOB: <job id>
#   LOG: <このホストから読めるログのパス>
#   NEXT: ジョブの走る場所（$PWD）と、違ったときの取り消し方
#   （ログの末尾 20 行。終わった直後なら、最後に TIME: job=<番号> elapsed=<実際の時間> estimate=<見積もり> <印>
#    class=<区分> ref=<参照> purpose=<目的> をログへ書き足すので、その行も含む。印は、-c のとき、timed で走らなかったとき、
#    止められていたときに付く。state=<終わったときの State>）
#   NEXT: （TIME 行に印があるときだけ）その時間を記録に使わないことと、測り直し方
#   RESULT: job=<番号> name=<名前> exit=<終了コード> state=<State>（& と wait で待った複数のジョブを見分ける。投げる前に
#           止めたときと sbatch が投げられなかったときも、job=- state=NOT_SUBMITTED で出す）
#   EXIT: <終了コード>（COMPLETED 以外で終わったときは、後ろに (State=<State>) を付ける）
# 終了コードはジョブの終了コード。ジョブが COMPLETED でなく（CANCELLED、TIMEOUT など）、sbatch が 0 を返したときは 1。
# 中断（INT、TERM、HUP）されたら、RESULT: の行（state=unknown か NOT_SUBMITTED）を出して 130 で終わる。
# 終わった時間と State は sacct から読む（State だけは、読めなければ scontrol。どちらも確かめられなければ 1）。記録が slurmdbd に届くまで SLURM_RUN_SACCT_TRIES 回（既定 10）、
# SLURM_RUN_SACCT_INTERVAL 秒（既定 1）おきに待ち、読めなければ警告して TIME 行を出さない。

. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script slurm-run "$@"
