# shellcheck shell=bash
# slurm-exclusive.sh — 排他の Slurm のジョブが走行中かの判定（source して使う）
#
# exclusive_slurm_job
#   排他のジョブ（squeue で OverSubscribe が NO の走行中のジョブ）があれば、最初の 1 つを
#   "<名前> <ジョブ番号>"（名前に空白があってもそのまま、番号は最後の語）で標準出力に出して 0 を返す。
#   無ければ、squeue が無い・失敗した・5 秒で終わらないときも、何も出さず 1 を返す。
#   claude-hooks の guard-exclusive-slurm（modules/home/parts/claude-hooks/src/rules/guard_exclusive_slurm.rs）と
#   同じ squeue の呼び方と判定にする（ADR-0041）。

exclusive_slurm_job() {
  command -v squeue >/dev/null 2>&1 || return 1
  local out line shares rest
  out=$(timeout 5 squeue -h -t R -o '%h|%j|%i' 2>/dev/null) || return 1
  while IFS= read -r line; do
    shares=${line%%|*}
    rest=${line#*|}
    [[ "$line" == *"|"*"|"* && "${shares// /}" == NO ]] || continue
    printf '%s %s\n' "${rest%|*}" "${rest##*|}"
    return 0
  done <<<"$out"
  return 1
}
