#!/usr/bin/env bash
# slurm-run.sh — r995 の Slurm に重い計算を 1 本投げ、終わるまで待つ
#
# Usage: slurm-run.sh [--top] [--timed | -c <コア数>] -t <見積もり> <name> <command>
#
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
# --top: 同じパーティションの待っているジョブより先に実行する（scontrol top。走っているジョブは止めない）
# --timed: 時間を記録するジョブ（と数分の急ぐ計算）。排他で、割り込まれないパーティション timed に投げる。待っている
#     main のジョブより先に走り、走っている main のジョブをすべて止めて実行し、自分は止められない。timed のジョブどうしは
#     止め合わず、投げた順に走る。止めたジョブはメモリに残る（Slurm はメモリを数えない）ので、走っているジョブとの合計が
#     搭載量に収まる計算に限る。-c と組めない（排他でない時間は記録に使えない）
# 連絡先として、git のブランチ、投げた場所、Claude Code のセッションの ID をジョブのコメントに書く
# （squeue の %k）。サブエージェントは親と同じセッションなので、ブランチで見分ける。
#
# ログは r995 の ~/github/slurm-logs/<name>-<job>.out。ジョブは投げたときのパスで r995 の上で走るので、
# r995 以外のホストでは /mnt/r995/github（r995 の ~/github の NFS。テストは SLURM_RUN_NFS_ENTRY で置き換える）の下で投げる。
#
# 出力:
#   JOB: <job id>
#   LOG: <このホストから読めるログのパス>
#   （ログの末尾 20 行。終わった直後なら、最後に TIME: job=<番号> elapsed=<実際の時間> estimate=<見積もり> を
#    ログへ書き足すので、その行も含む。-c のとき、timed で走らなかったとき、止められていたときは、行末に印が付く）
#   EXIT: <ジョブの終了コード>
# 終了コードはジョブの終了コード。

set -euo pipefail

usage() {
  echo "Usage: $0 [--top] [--timed | -c <コア数>] -t <見積もり> <name> <command>" >&2
  exit 1
}

top=false
timed=false
estimate=""
cpus=""
shared=false
while (($#)); do
  case "$1" in
    --top) top=true ;;
    --timed) timed=true ;;
    -t)
      (($# >= 2)) || usage
      estimate=$2
      shift
      ;;
    -c)
      (($# >= 2)) || usage
      cpus=$2
      shared=true
      shift
      ;;
    -*)
      echo "ERROR: 知らないオプション: $1（時間を記録するジョブと急ぐ計算は --timed）" >&2
      exit 1
      ;;
    *) break ;;
  esac
  shift
done
[[ -n $estimate ]] || usage
(($# == 2)) || usage
name=$1
command=$2

if $timed && $shared; then
  echo "ERROR: --timed は -c と組めない（排他でないジョブの時間は記録に使えない）" >&2
  exit 1
fi

if $shared && [[ ! $cpus =~ ^[1-9][0-9]*$ ]]; then
  echo "ERROR: -c のコア数は正の整数: '$cpus'" >&2
  exit 1
fi

if [[ ! $name =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "ERROR: 名前はファイル名に使える英数字と . _ - だけにする: $name" >&2
  exit 1
fi

# slurmd が書く r995 の側のパスと、このホストから読むパス
remote_logs="$HOME/github/slurm-logs"
if [[ $(hostname) == r995 ]]; then
  local_logs=$remote_logs
else
  nfs_entry=${SLURM_RUN_NFS_ENTRY:-/mnt/r995/github}
  case "$PWD/" in
    "$nfs_entry/"*) ;;
    *)
      echo "ERROR: r995 以外では $nfs_entry の下で投げる（ジョブは投げたときのパスで r995 の上で走る）: $PWD" >&2
      exit 1
      ;;
  esac
  local_logs="$nfs_entry/slurm-logs"
fi
mkdir -p "$local_logs"

contact=$PWD
if branch=$(git symbolic-ref --short -q HEAD 2>/dev/null); then
  contact="$branch $PWD"
fi
if [[ -n ${CLAUDE_CODE_SESSION_ID:-} ]]; then
  contact+=" claude=$CLAUDE_CODE_SESSION_ID"
fi
# 排他でないジョブが走っていると、排他のジョブが始まらない理由を squeue の %k で読めるようにする
resources=(--exclusive)
if $shared; then
  resources=(--cpus-per-task="$cpus")
  contact+=" shared-cpus=$cpus"
fi
# 既定の main も明示する。コマンド行の --partition は SBATCH_PARTITION より強く、環境から timed に入った -c のジョブが
# 時間を記録するジョブとして数えられることを防ぐ
if $timed; then
  resources+=(--partition=timed)
else
  resources+=(--partition=main)
fi

# sbatch --wait は終わるまで戻らないので、裏で実行し、最初の行から番号を読む
submitted=$(mktemp)
trap 'rm -f "$submitted"' EXIT
sbatch "${resources[@]}" --wait --export=ALL -J "$name" -o "$remote_logs/%x-%j.out" \
  --time "$estimate" --comment "$contact" --wrap "$command" >"$submitted" &
pid=$!
job=""
while [[ -z $job ]]; do
  if read -r line <"$submitted" && [[ $line =~ ^Submitted\ batch\ job\ ([0-9]+) ]]; then
    job=${BASH_REMATCH[1]}
  elif ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid" || true
    echo "ERROR: sbatch がジョブを投げられなかった:" >&2
    cat "$submitted" >&2
    exit 1
  else
    sleep 1
  fi
done
echo "JOB: $job"
log="$local_logs/$name-$job.out"
echo "LOG: $log"
# 並べ替えに失敗しても、投げたジョブは待ち行列にあるので、終わりまで待つ
if $top && ! scontrol top "$job"; then
  echo "WARN: ジョブ $job を待ち行列の先頭に移せなかった。投げた順に走る" >&2
fi

status=0
wait "$pid" || status=$?
# 終わった直後なら、slurmctld がジョブを覚えている（MinJobAge の間）。見積もりと実際の時間を
# ログの末尾に書き足し、tail で一緒に見せる。ここで失敗しても、ジョブの終了コードは返す。
# RunTime と TimeLimit は scontrol が隣り合わせに出す。後ろの行の Comment や WorkDir は利用者が
# 決める値なので、.* でまたいで拾わない
if ! info=$(scontrol show job "$job"); then
  echo "WARN: ジョブ $job の実際の時間を scontrol から読めなかった" >&2
elif [[ $info =~ RunTime=([^ ]+)\ TimeLimit=([^ ]+) ]]; then
  time_line="TIME: job=$job elapsed=${BASH_REMATCH[1]} estimate=${BASH_REMATCH[2]}"
  if $shared; then
    time_line+=" shared (-c $cpus): 時間は当てにならない"
  fi
  # 止められたかによらず付ける。止められなかったジョブも、割り込まれうる条件で走った値で、付け忘れた --timed を見分けるため。
  # flag でなく実際に走ったパーティションで決める（SBATCH_PARTITION などで変わりうる。slurm-history.sh も jobcomp の
  # Partition で決める）。前に出る欄は固定の欄と JobName だけなので、最初の「空白 + Partition=」が本物の欄
  if [[ ! $info =~ [[:space:]]Partition=([^[:space:]]+) ]]; then
    echo "WARN: ジョブ $job の scontrol の出力に Partition が見つからず、割り込まれうるかを確かめられなかった" >&2
    time_line+=" partition=unknown: 割り込まれうるかを確かめられないので時間は記録に使わない"
  elif [[ ${BASH_REMATCH[1]} != timed ]]; then
    time_line+=" preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed）"
  fi
  # timed のジョブは割り込まれないので、この印が付けば設定か運用（scontrol suspend）の誤り。
  # RunTime は止められていた時間を除くが、再開後のキャッシュの冷えや、プログラムが自分で測る経過時間は残る。
  # SuspendTime は一度も止められていなければ None。前に出る欄は固定の欄と JobName（slurm-run.sh は空白を
  # 許さない）だけなので、最初の「空白 + SuspendTime=」が本物の欄で、後ろの Comment などの値は拾わない
  if [[ ! $info =~ [[:space:]]SuspendTime=([^[:space:]]+) ]]; then
    echo "WARN: ジョブ $job の scontrol の出力に SuspendTime が見つからず、止められていたかを確かめられなかった（sacct の Suspended で確かめる）" >&2
    time_line+=" suspended=unknown: 止められたかを確かめられないので時間は当てにならない"
  elif [[ ${BASH_REMATCH[1]} != None ]]; then
    time_line+=" suspended: 止められていたので時間は当てにならない"
  fi
  echo "$time_line" >>"$log" || {
    echo "WARN: ジョブ $job の実際の時間をログに書き足せなかった: $log" >&2
    echo "$time_line"
  }
else
  echo "WARN: ジョブ $job の scontrol の出力に RunTime と TimeLimit が見つからなかった" >&2
fi
tail -n 20 "$log" 2>/dev/null || true
echo "EXIT: $status"
exit "$status"
