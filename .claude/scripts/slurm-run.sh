#!/usr/bin/env bash
# slurm-run.sh — r995 の Slurm に重い計算を 1 本投げ、終わるまで待つ
#
# Usage: slurm-run.sh [--top] -t <見積もり> <name> <command>
#
# ほかの計算と重ならないよう排他で走らせる（sbatch --exclusive --wait --export=ALL）。
# <name> はジョブの名前（squeue で見分ける）とログのファイル名に使う。例: xlc-2G、openmc-2G
# -t: かかる時間の見積もり（sbatch --time の形。分か H:MM:SS）。待ち行列の見込み（squeue --start）に
#     使う。超えても止めない（slurm.conf の OverTimeLimit=UNLIMITED）
# --top: 待っているジョブより先に走らせる（scontrol top。走っているジョブは止めない）
# 連絡先として、git のブランチ、投げた場所、Claude Code のセッションの ID をジョブのコメントに書く
# （squeue の %k）。サブエージェントは親と同じセッションなので、ブランチで見分ける。
#
# ログは r995 の ~/github/slurm-logs/<name>-<job>.out。ジョブは投げたときのパスで r995 の上で走るので、
# r995 以外のホストでは ~/r995（r995 の ~/github の NFS）の下で投げる。
#
# 出力:
#   JOB: <job id>
#   LOG: <このホストから読めるログのパス>
#   （ログの末尾 20 行）
#   EXIT: <ジョブの終了コード>
# 終了コードはジョブの終了コード。

set -euo pipefail

usage() {
  echo "Usage: $0 [--top] -t <見積もり> <name> <command>" >&2
  exit 1
}

top=false
estimate=""
while (($#)); do
  case "$1" in
    --top) top=true ;;
    -t)
      (($# >= 2)) || usage
      estimate=$2
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

if [[ ! $name =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "ERROR: 名前はファイル名に使える英数字と . _ - だけにする: $name" >&2
  exit 1
fi

# slurmd が書く r995 の側のパスと、このホストから読むパス
remote_logs="$HOME/github/slurm-logs"
if [[ $(hostname) == r995 ]]; then
  local_logs=$remote_logs
else
  case "$PWD/" in
    "$HOME/r995/"*) ;;
    *)
      echo "ERROR: r995 以外では ~/r995 の下で投げる（ジョブは投げたときのパスで r995 の上で走る）: $PWD" >&2
      exit 1
      ;;
  esac
  local_logs="$HOME/r995/slurm-logs"
fi
mkdir -p "$local_logs"

contact=$PWD
if branch=$(git symbolic-ref --short -q HEAD 2>/dev/null); then
  contact="$branch $PWD"
fi
if [[ -n ${CLAUDE_CODE_SESSION_ID:-} ]]; then
  contact+=" claude=$CLAUDE_CODE_SESSION_ID"
fi

# sbatch --wait は終わるまで戻らないので、裏で走らせ、最初の行から番号を読む
submitted=$(mktemp)
trap 'rm -f "$submitted"' EXIT
sbatch --exclusive --wait --export=ALL -J "$name" -o "$remote_logs/%x-%j.out" \
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
tail -n 20 "$log" 2>/dev/null || true
echo "EXIT: $status"
exit "$status"
