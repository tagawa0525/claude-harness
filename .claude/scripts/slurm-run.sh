#!/usr/bin/env bash
# slurm-run.sh — r995 の Slurm に重い計算を 1 本投げ、終わるまで待つ
#
# Usage: slurm-run.sh [--top] [--urgent] [-c <コア数>] -t <見積もり> <name> <command>
#
# 既定では、ほかの計算と重ならないよう排他で実行する（sbatch --exclusive --wait --export=ALL）。
# <name> はジョブの名前（squeue で見分ける）とログのファイル名に使う。例: xlc-2G、openmc-2G
# -t: かかる時間の見積もり（sbatch --time の形。分か H:MM:SS）。待ち行列の見込み（squeue --start）に
#     使う。超えても止めない（slurm.conf の OverTimeLimit=UNLIMITED）
# -c: 排他にせず、<コア数> の CPU（Slurm の CPUs。ハードウェアスレッドで数え、割り当ては物理コア単位に
#     切り上がる）だけを取って実行する（--cpus-per-task）。時間を記録しない長い計算のためで、ほかのジョブと
#     資源を取り合うので、TIME 行に「時間は当てにならない」印を付ける。排他のジョブはノードにジョブが
#     走っている間は始まらないので、走っている間は待たせる
# --top: 待っているジョブより先に実行する（scontrol top。走っているジョブは止めない）
# --urgent: パーティション urgent に投げる。待っている main のジョブより先に走り、走っている main のジョブのうち
#     コアの重なるものを止めて（SUSPEND）実行する。排他（-c なし）ならノード全体を取るので、走っている main のジョブを
#     すべて止める。止められたジョブは、urgent のジョブが終わると続きから再開する。
#     止めたジョブはメモリに残る（Slurm はメモリを数えない）ので、走っているジョブとの合計が搭載量に収まる計算に限る
# 連絡先として、git のブランチ、投げた場所、Claude Code のセッションの ID をジョブのコメントに書く
# （squeue の %k）。サブエージェントは親と同じセッションなので、ブランチで見分ける。
#
# ログは r995 の ~/github/slurm-logs/<name>-<job>.out。ジョブは投げたときのパスで r995 の上で走るので、
# r995 以外のホストでは /mnt/r995/github（r995 の ~/github の NFS。テストは SLURM_RUN_NFS_ENTRY で置き換える）の下で投げる。
#
# 出力:
#   JOB: <job id>
#   LOG: <このホストから読めるログのパス>
#   （ログの末尾 20 行。終わった直後なら、最後に TIME: elapsed=<実際の時間> estimate=<見積もり> を
#    ログへ書き足すので、その行も含む。-c のとき、urgent のジョブに止められていたときは、行末に印が付く）
#   EXIT: <ジョブの終了コード>
# 終了コードはジョブの終了コード。

set -euo pipefail

usage() {
  echo "Usage: $0 [--top] [--urgent] [-c <コア数>] -t <見積もり> <name> <command>" >&2
  exit 1
}

top=false
urgent=false
estimate=""
cpus=""
shared=false
while (($#)); do
  case "$1" in
    --top) top=true ;;
    --urgent) urgent=true ;;
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
    *) break ;;
  esac
  shift
done
[[ -n $estimate ]] || usage
(($# == 2)) || usage
name=$1
command=$2

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
if $urgent; then
  resources+=(--partition=urgent)
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
  time_line="TIME: elapsed=${BASH_REMATCH[1]} estimate=${BASH_REMATCH[2]}"
  if $shared; then
    time_line+=" shared (-c $cpus): 時間は当てにならない"
  fi
  # RunTime は止められていた時間を除くが、再開後のキャッシュの冷えや、プログラムが自分で測る経過時間は残る。
  # SuspendTime は一度も止められていなければ None。前に出る欄は固定の欄と JobName（slurm-run.sh は空白を
  # 許さない）だけなので、最初の「空白 + SuspendTime=」が本物の欄で、後ろの Comment などの値は拾わない
  if [[ ! $info =~ [[:space:]]SuspendTime=([^[:space:]]+) ]]; then
    echo "WARN: ジョブ $job の scontrol の出力に SuspendTime が見つからず、止められていたかを確かめられなかった（sacct の Suspended で確かめる）" >&2
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
