#!/usr/bin/env bash
# slurm-queue.sh — Slurm の待ち行列を、slurm-run.sh がコメントに入れた区分と目的つきで、走る順に並べる
#
# Usage: slurm-queue.sh
#
# 出力（ジョブごとに 3 行。走る順）:
#   <番号> <状態> <パーティション> <優先度> <区分> <見積もり> <開始の見込み> <名前> <参照>
#     目的: <結果が何を決めるか>
#     連絡先: <ブランチ、投げた場所、claude= セッション、shared-cpus=>
#   ORDER: <パーティション>: <番号>（<区分>）が <番号>（<区分>）の後ろ、…で待っている。scontrol top <番号>,<番号>,…[（区分の無い …）]
#
# 走る順は、パーティション timed（PriorityTier が高い）、main の順で、その中は走っているもの（走り始めた CONFIGURING
# なども）、止められたもの、待っているもの（優先度の高い順、同じなら番号の順）。区分の順は timed、record、decision、
# trial で、同じパーティションで待っているジョブのうち、上の区分のジョブが下の区分のジョブより後ろにあれば、パーティション
# ごとに ORDER: の行を 1 つ出す。後ろにある上の区分のジョブごとに、前にある下の区分のジョブのうち最も近いものを挙げる。
# 並べるコマンドは、区分のある待っているジョブをすべて、区分の順（同じ区分は今の順）に並べた 1 つの scontrol top に
# する。scontrol top は渡したジョブをその順に先頭へ出すので、ジョブごとに分けて実行すると最後に出したジョブが先頭に
# なり、1 つだけを出すと、その前にいた上の区分のジョブも追い越す。
# 走っているジョブは止めないので比べない。区分の無いジョブ（slurm-run.sh の外から投げたもの）は区分を - にし、
# 順を比べず、並べるコマンドにも入れない（何を決めるかが分からない）。並べるコマンドで区分のあるジョブの後ろに回るので、
# 待っている区分の無いジョブを ORDER: の行の末尾に「（区分の無い <番号>,… は後ろに回る）」で挙げる。
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
awk -F'|' '{
  part_key = ($2 == "timed") ? 0 : 1
  state_key = ($4 == "PENDING") ? 2 : ($4 == "SUSPENDED") ? 1 : 0
  printf "%d\t%d\t%d\t%d\t%s\n", part_key, state_key, -$5, $1, $0
}' <<<"$lines" | sort -t$'\t' -k1,1n -k2,2n -k3,3n -k4,4n | cut -f5- | awk -F'|' '
BEGIN {
  rank["timed"] = 1
  rank["record"] = 2
  rank["decision"] = 3
  rank["trial"] = 4
}
{
  job = $1; part = $2; name = $3; state = $4; prio = $5; limit = $6; start = $7
  # 正規表現の {n} は mawk の古い版が解さないので、欄をつなぎ直す
  comment = $8
  for (i = 9; i <= NF; i++) comment = comment "|" $i
  class = "-"; ref = "-"; purpose = "（無い）"; contact = comment
  n = split(comment, f, / \| /)
  if (n >= 4 && (f[1] in rank)) {
    class = f[1]; ref = f[2]; purpose = f[3]
    for (i = 1; i <= 3; i++) sub(/^[^|]* \| /, "", contact)
  }
  printf "%s %s %s %s %s %s %s %s %s\n", job, state, part, prio, class, limit, start, name, ref
  printf "  目的: %s\n", purpose
  printf "  連絡先: %s\n", contact
  if (state != "PENDING") next
  if (!(part in seen)) {
    seen[part] = 1
    parts[++nparts] = part
  }
  if (class == "-") {
    unclassed[part] = unclassed[part] (unclassed[part] == "" ? "" : ",") job
    next
  }
  # 同じパーティションで先に並ぶ、より下の区分のジョブのうち最も近いもの
  for (k = count[part]; k >= 1; k--) {
    if (rank[pend_class[part, k]] > rank[class]) {
      behind[part] = behind[part] (behind[part] == "" ? "" : "、") sprintf("%s（%s）が %s（%s）の後ろ", job, class, pend_job[part, k], pend_class[part, k])
      break
    }
  }
  count[part]++
  pend_job[part, count[part]] = job
  pend_class[part, count[part]] = class
}
END {
  for (p = 1; p <= nparts; p++) {
    part = parts[p]
    if (behind[part] == "") continue
    # 区分の順、同じ区分は今の順に並べる（挿入ソート。待っているジョブは数本）
    m = count[part]
    for (i = 1; i <= m; i++) idx[i] = i
    for (i = 2; i <= m; i++) {
      v = idx[i]
      for (j = i - 1; j >= 1 && rank[pend_class[part, idx[j]]] > rank[pend_class[part, v]]; j--) idx[j + 1] = idx[j]
      idx[j + 1] = v
    }
    list = ""
    for (i = 1; i <= m; i++) list = list (i == 1 ? "" : ",") pend_job[part, idx[i]]
    note = (unclassed[part] == "") ? "" : sprintf("（区分の無い %s は後ろに回る）", unclassed[part])
    printf "ORDER: %s: %sで待っている。scontrol top %s%s\n", part, behind[part], list, note
  }
}'
