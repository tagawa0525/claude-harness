# shellcheck shell=bash
# slurm-exclusive.sh — 排他の Slurm のジョブが実行中かの判定（source して使う）
#
# exclusive_slurm_job
#   排他のジョブ（squeue で OverSubscribe が NO の実行中のジョブ。列は OverSubscribe|名前|番号|ノード）があれば、
#   "<here|away> <名前> <番号>"（名前に空白があってもそのまま、番号は最後の語）を標準出力に出して 0 を返す。このホストが
#   どれかの排他のジョブのノードなら here とそのジョブ、でなければ away と最初のジョブ。名前はドメインの前を大文字小文字に
#   よらず比べ、hostname が失敗したとき、ノードの一覧を読めないとき（縮めた形の node[1-3]）は here（止める側に倒す）。
#   無ければ、squeue が無い・失敗した・5 秒で終わらないときも、何も出さず 1 を返す。
#   claude-hooks の guard-exclusive-slurm（modules/home/parts/claude-hooks/src/rules/guard_exclusive_slurm.rs）と
#   同じ squeue の呼び方と判定にする。

exclusive_slurm_job() {
  command -v squeue >/dev/null 2>&1 || return 1
  local out line shares rest name id nodes node host first=""
  out=$(timeout 5 squeue -h -t R -o '%h|%j|%i|%N' 2>/dev/null) || return 1
  host=$(hostname 2>/dev/null) || host=""
  host=${host%%.*}
  host=${host,,}
  while IFS= read -r line; do
    line=${line%$'\r'}
    shares=${line%%|*}
    [[ "$line" == *"|"*"|"*"|"* && "${shares// /}" == NO ]] || continue
    rest=${line#*|}
    nodes=${rest##*|}
    nodes=${nodes// /}
    rest=${rest%|*}
    id=${rest##*|}
    id=${id// /}
    name=${rest%|*}
    [[ -n "$first" ]] || first="$name $id"
    if [[ -z "$host" || -z "$nodes" || "$nodes" == *"["* ]]; then
      printf 'here %s %s\n' "$name" "$id"
      return 0
    fi
    local IFS=,
    for node in $nodes; do
      node=${node%%.*}
      if [[ "${node,,}" == "$host" ]]; then
        printf 'here %s %s\n' "$name" "$id"
        return 0
      fi
    done
    unset IFS
  done <<<"$out"
  [[ -n "$first" ]] || return 1
  printf 'away %s\n' "$first"
}
