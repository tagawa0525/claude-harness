#!/usr/bin/env bash
# slurm-queue.sh — Slurm の待ち行列を、slurm-run.sh がコメントに入れた区分と目的つきで、走る順に並べる
#
# Usage: slurm-queue.sh
#
# 出力（ジョブごとに 3 行。走る順）:
#   <番号> <状態> <パーティション> <優先度> <区分> <見積もり> <開始の見込み> <名前> <参照>
#     目的: <結果が何を決めるか>
#     連絡先: <ブランチ、投げた場所、claude= セッション、shared-cpus=>
#   ORDER: <番号>（<区分>）が <番号>（<区分>）の後ろで待っている。scontrol top <番号> で前に出す
#
# 走る順は、パーティション timed（PriorityTier が高い）、main の順で、その中は走っているもの、止められたもの、
# 待っているもの（優先度の高い順、同じなら番号の順）。区分の順は timed、record、decision、trial で、同じパーティションで
# 待っているジョブのうち、上の区分のジョブが下の区分のジョブより後ろにあれば ORDER: の行を、待ち行列の順に出す
# （上から順に scontrol top を実行すると、区分の順に並ぶ）。後ろにある下の区分のジョブのうち最も近いものを挙げる。
# 走っているジョブは止めないので比べない。区分の無いジョブ（slurm-run.sh の外から投げたもの）は区分を - にし、
# 順を比べない（何を決めるかが分からない）。
#
# コメントの形は slurm-run.sh が書く「<区分> | <参照> | <目的> | <連絡先>」。参照と目的は | を含まないので、
# 最初の 3 つの区切りまでを分け、残りを連絡先にする。

set -euo pipefail

if (($#)); then
  echo "Usage: $0" >&2
  exit 1
fi

# 名前は slurm-run.sh が英数字と . _ - に限り、ほかの固定の欄も | を含まないので、最後の欄（コメント）の前までを | で分けられる
status=0
lines=$(squeue --noheader --format '%i|%P|%j|%T|%Q|%l|%S|%k') || status=$?
if ((status != 0)); then
  echo "ERROR: squeue が失敗した（終了コード $status）" >&2
  exit "$status"
fi
if [[ -z $lines ]]; then
  echo "待ち行列にジョブが無い"
  exit 0
fi

# 並べ替えの鍵（パーティション、状態、優先度、番号）を前に付けて sort し、鍵を外して読む
awk -F'|' '
{
  comment = $8
  for (i = 9; i <= NF; i++) comment = comment "|" $i
  part_key = ($2 == "timed") ? 0 : 1
  state_key = ($4 == "RUNNING") ? 0 : ($4 == "SUSPENDED") ? 1 : ($4 == "PENDING") ? 2 : 3
  printf "%d\t%d\t%d\t%d\t%s|%s|%s|%s|%s|%s|%s|%s\n", part_key, state_key, -$5, $1, $1, $2, $3, $4, $5, $6, $7, comment
}' <<<"$lines" | sort -t$'\t' -k1,1n -k2,2n -k3,3n -k4,4n | cut -f5- | awk -F'|' '
BEGIN {
  rank["timed"] = 1
  rank["record"] = 2
  rank["decision"] = 3
  rank["trial"] = 4
}
{
  job = $1; part = $2; name = $3; state = $4; prio = $5; limit = $6; start = $7
  comment = $8
  for (i = 9; i <= NF; i++) comment = comment "|" $i
  class = "-"; ref = "-"; purpose = "（無い）"; contact = comment
  n = split(comment, f, / \| /)
  if (n >= 4 && (f[1] in rank)) {
    class = f[1]; ref = f[2]; purpose = f[3]
    # 連絡先は 4 つ目の区切りの後のすべて
    contact = substr(comment, length(f[1]) + length(f[2]) + length(f[3]) + 10)
  }
  printf "%s %s %s %s %s %s %s %s %s\n", job, state, part, prio, class, limit, start, name, ref
  printf "  目的: %s\n", purpose
  printf "  連絡先: %s\n", contact
  if (state == "PENDING" && class != "-") {
    # 同じパーティションで先に並ぶ、より下の区分のジョブのうち最も近いもの
    for (k = count[part]; k >= 1; k--) {
      if (rank[pend_class[part, k]] > rank[class]) {
        order[++orders] = sprintf("ORDER: %s（%s）が %s（%s）の後ろで待っている。scontrol top %s で前に出す", job, class, pend_job[part, k], pend_class[part, k], job)
        break
      }
    }
    count[part]++
    pend_job[part, count[part]] = job
    pend_class[part, count[part]] = class
  }
}
END {
  for (i = 1; i <= orders; i++) print order[i]
}'
