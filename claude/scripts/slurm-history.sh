#!/bin/sh
# slurm-history.sh — jobcomp と sacct の記録から、ジョブの見積もり（--time）と実績とメモリの最大を比べる。--job は sacct から
# 1 本ずつ読む。中身は claude-hooks script slurm-history（判定を、起動せずに Rust の単体のテストで確かめるため）。
# スキルの文と許可の設定はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: slurm-history.sh [--by-name] [--sacct] [FILE]
#        slurm-history.sh --job [--repo <本体のリポジトリ>] <ジョブ番号>...
#   FILE       jobcomp/filetxt のファイル。省略時は /var/log/slurm/job_completions
#   --sacct    sacct の記録も読む（FILE を省略したときは常に読む。FILE だけを渡したときは jobcomp だけで、sacct を呼ばない）
#   --by-name  ジョブ名ごとに集計する（省略時はジョブごと）
#   --job      ジョブごとに、状態、時間、排他か、コマンド、作業ディレクトリ、走った版（決められなければ理由）を
#              sacct の記録と git の reflog から出す（下の「--job」）
#
# 出力（表のあとに 3 行と、当たるときだけ NEXT:）:
#   ジョブごと: JOBID NAME ESTIMATE ELAPSED RATIO MAX_RSS
#   ジョブ名ごと: NAME RUNS ESTIMATE ELAPSED RATIO MAX_RATIO MAX_RSS（ESTIMATE と ELAPSED は平均。
#   見積もりのあるジョブだけで求め、RUNS は見積もりのないジョブも数える。UNTIMED のジョブは数えない）
#   SKIPPED: <最後まで走っていないので表に入れなかった行数>
#   UNTIMED: <割り込みを入れた後に終わった、timed でないので表に入れなかったジョブの数>
#   SHARED: <排他でないので表に入れなかった sacct のジョブの数>
#   NEXT: <UNTIMED か SHARED が 0 でないときだけ、そのジョブの実績を sacct で読むコマンド>
#
# 定義:
#   ELAPSED  EndTime - StartTime（jobcomp に Elapsed の欄は無い）
#   ESTIMATE TimeLimit の分を秒にしたもの。数値でない値（UNLIMITED など）は見積もりなし
#   RATIO    ELAPSED / ESTIMATE。1 を超えれば見積もりより長くかかった。見積もりなしは -
#            ジョブ名ごとの RATIO は ELAPSED の合計 / ESTIMATE の合計、MAX_RATIO は個々の比率の最大
# 対象: JobState=COMPLETED かつ NodeCnt が 1 以上の行。それ以外（開始前に CANCELLED、
#       FAILED、TIMEOUT など、最後まで走っていない実績）は SKIPPED に数えて表に入れない。
#       割り込みを入れた時刻（2026-10-23T21:40:00）より後に終わったジョブは、割り込まれないパーティション timed のもの
#       （slurm-run.sh --timed）だけを表に入れ、ほかのパーティションのものは UNTIMED に数える。main のジョブは、止められ
#       なかったときも割り込まれうる条件で走った時間なので入れない。timed の前の urgent のジョブは、排他でないものと
#       見分けられないので入れない。それより前に終わったジョブは割り込みが無かったので
#       パーティションによらず表に入れる。
#
#   MAX_RSS  sacct のジョブのステップ（.batch など）の MaxRSS の最大。jobcomp は持たないので、sacct に無いジョブは -。
#            ジョブ名ごとは、ジョブの MAX_RSS の最大。M（MiB）か G（GiB）
# sacct: 2026-10-12（sacct の記録を始めた日）以降を読む。jobcomp にも sacct にもある JobId は sacct のほうを使う
#       （jobcomp の行を数えない）。SubmitLine の sbatch の直後の語が --cpus-per-task=N のジョブ（slurm-run.sh -c）は
#       排他でないので、時間が当てにならず、表に入れずに SHARED に数える。--exclusive か、読めない（slurm-run.sh を
#       通らない）ジョブは入れる。2026-10-12 より前の jobcomp は排他かを持たないので、名前の -shared で分ける。
#
# jobcomp の行は `キー=値` の空白区切り。値に `=` や `,` を含む欄（Tres）があるので最初の `=` で分ける。
# `=` を含まない語は直前の値の続き（空白のあるジョブ名）として読む。
# 読めない行（JobId、JobState、TimeLimit、NodeCnt、時刻、割り込みの後に終わった行の Partition のどれかが無いか
# 壊れている）は、行番号を出して exit 1。ファイルが読めないときと、sacct が失敗したときも exit 1。
#
# --job:
#   ジョブごとに次の行を出し、ジョブの間を空行で区切る。
#     JOB: <番号> <名前>
#     STATE: / ELAPSED: / TIMES: submit=<Submit> start=<Start> end=<End> / WORKDIR:   sacct の欄のまま
#     EXCLUSIVE: yes | no (--cpus-per-task=N) | unknown (<sbatch の直後の語>)
#     BRANCH: / COMMIT:   SubmitLine の --comment の連絡先のブランチと commit=（-dirty を含む）。無ければ -
#     REPO: <本体のリポジトリ> | -
#     VERSION: <完全な hash> | <hash>（reflog の <ブランチ>@{<時刻>}） | -
#     REASON: <版を決められない理由>   VERSION が - のとき、1 行に 1 つ
#     NOTE: <版を決めたが確かめていないこと>
#     COMMAND: <--wrap の後の 1 行目>、COMMAND+ <2 行目以降>
#   sacct に無い番号は ERROR を出し、ほかの番号を出した後に 1 を返す。
#
#   版の決め方:
#     commit= があれば、そのコミット（本体に無ければ、-dirty なら決めない）。版を取った時点は Submit。
#     無ければ、ブランチの reflog の Start 以前の最後の項のコミット。版を取った時点は Start。
#     版を取った時点から End（走っている間は今）までに、ブランチの reflog か、残っている worktree の HEAD の reflog に、
#     版と違うコミットへ移る項があれば決めない。
#   本体のリポジトリは --repo、残っている WorkDir の git の共通のディレクトリ、worktree-add.sh の名前
#   （<本体>-<ブランチの / を - に>）の順に探す。worktree は後片付けで消えるが、reflog は本体の共通の git dir に残る。
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script slurm-history "$@"
