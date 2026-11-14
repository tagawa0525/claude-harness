# shellcheck shell=bash
# slurm-exclusive.sh — 排他の Slurm のジョブが実行中かの判定（source して使う）
#
# exclusive_slurm_job
#   排他のジョブ（squeue で OverSubscribe が NO の実行中のジョブ。列は OverSubscribe|名前|番号|ノード）があれば、最初の 1 つを
#   "<名前> <ジョブ番号>"（名前に空白があってもそのまま、番号は最後の語）で標準出力に出して 0 を返す。
#   無ければ、squeue が無い・失敗した・5 秒で終わらないときも、何も出さず 1 を返す。
#   claude-hooks の guard-exclusive-slurm（modules/home/parts/claude-hooks/src/rules/guard_exclusive_slurm.rs）と
#   同じ squeue の呼び方と判定にする。

exclusive_slurm_job() {
  command -v squeue >/dev/null 2>&1 || return 1
  local out line shares rest
  out=$(timeout 5 squeue -h -t R -o '%h|%j|%i|%N' 2>/dev/null) || return 1
  while IFS= read -r line; do
    shares=${line%%|*}
    rest=${line#*|}
    [[ "$line" == *"|"*"|"*"|"* && "${shares// /}" == NO ]] || continue
    # 名前に | があっても、ノードと番号は後ろから取る
    rest=${rest%|*}
    printf '%s %s\n' "${rest%|*}" "${rest##*|}"
    return 0
  done <<<"$out"
  return 1
}

# on_compute_node
#   このホストが、実行中の排他のジョブのノード（squeue の %N）なら 0。名前はドメインの前を大文字小文字によらず比べる。
#   hostname が失敗したとき、ノードの一覧を読めないとき（縮めた形の node[1-3]、squeue の失敗）も 0（止める側に倒す）。
#   claude-hooks の guard-exclusive-slurm の ExclusiveJob::runs_on と同じ判定にする
on_compute_node() {
  local host out line nodes node
  host=$(hostname 2>/dev/null) || return 0
  host=${host%%.*}
  host=${host,,}
  [[ -n "$host" ]] || return 0
  out=$(timeout 5 squeue -h -t R -o '%h|%N' 2>/dev/null) || return 0
  while IFS= read -r line; do
    [[ "${line%%|*}" == NO ]] || continue
    nodes=${line##*|}
    [[ -n "$nodes" && "$nodes" != *"["* ]] || return 0
    local IFS=,
    for node in $nodes; do
      node=${node%%.*}
      [[ "${node,,}" == "$host" ]] && return 0
    done
  done <<<"$out"
  return 1
}
