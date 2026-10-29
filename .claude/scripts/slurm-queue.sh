#!/usr/bin/env bash
# slurm-queue.sh — Slurm の待ち行列を、slurm-run.sh がコメントに入れた区分と目的つきで、走る順に並べる
#
# Usage: slurm-queue.sh                           # 一覧
#        slurm-queue.sh --order [--first <番号>]  # main を区分の順に並べる番号の列（slurm-run.sh が投げた後に使う）
#
# 一覧（ジョブごとに 3 行。走る順）:
#   <番号> <状態>[（動かせない）] <パーティション> <優先度> <区分> <見積もり> <開始の見込み> <名前> <参照>
#     目的: <結果が何を決めるか>
#     連絡先: <ブランチ、投げた場所、claude= セッション、shared-cpus=>
#   ORDER: main: <番号>（<区分>）が <番号>（<区分>）の後ろ、…で待っている。scontrol top <番号>,<番号>,…[（区分の無い …）]
#
# 走る順は、パーティション timed（PriorityTier が高い）、main の順で、その中は走っているもの（走り始めた CONFIGURING
# なども）、止められたもの、待っているもの（優先度の高い順、同じなら番号の順）。
#
# 区分の順（timed、record、decision、trial）で並べ直すのはパーティション main の待っているジョブだけ。timed のジョブは、
# 時間の記録と、走っているジョブの終わりを待てない数分の計算で、投げた順に実行する（区分の順で並べると、急ぐ数分の計算が
# 長い測定の後ろに回る）。動かせないジョブ（優先度が 1 以下の保留のものと、配列の親ジョブ。scontrol top が受け付けない）と
# 区分の無いジョブ（slurm-run.sh の外から投げたもの。何を決めるかが分からない）は、比べず、列にも入れない。
#
# 並べる列は、区分のある動かせる待っているジョブをすべて、区分の順、同じ区分は今の順に並べたもの（--first のジョブは
# 自分の区分の先頭）。scontrol top は渡したジョブをその順に先頭へ出すので、1 つの列で渡す（ジョブごとに分けると最後に
# 出したジョブが先頭になり、1 本だけ出すと、その前にいた上の区分のジョブも追い越す）。列が今の順と同じで、区分の無い
# 待っているジョブが列のジョブより前にいなければ、一覧は ORDER: を出さず、--order は何も出さない。列に入れないジョブは列のジョブの後ろに回るので、区分の無い待っているジョブを
# ORDER: の行の末尾に挙げる。
#
# コメントの形は slurm-run.sh が書く「<区分> | <参照> | <目的> | <連絡先>」。参照と目的は | を含まないので、
# 最初の 3 つの区切りまでを分け、残りを連絡先にする。

set -euo pipefail

usage() {
  echo "Usage: $0 [--order [--first <番号>]]" >&2
  exit 1
}

order=0
first=""
if (($#)); then
  [[ $1 == --order ]] || usage
  order=1
  shift
  if (($#)); then
    if [[ $1 != --first ]] || (($# != 2)); then usage; fi
    first=$2
  fi
fi

# 名前は slurm-run.sh が英数字と . _ - に限り、ほかの固定の欄も | を含まないので、最後の欄（コメント）の前までを | で分けられる
status=0
lines=$(squeue --noheader --format '%i|%P|%j|%T|%Q|%l|%S|%k') || status=$?
if ((status != 0)); then
  echo "ERROR: squeue が失敗した（終了コード $status）" >&2
  exit "$status"
fi
if [[ -z $lines ]]; then
  ((order)) || echo "待ち行列にジョブが無い"
  exit 0
fi

# 並べ替えの鍵（パーティション、状態、優先度、番号）を前に付けて sort し、鍵を外して読む
awk -F'|' '{
  part_key = ($2 == "timed") ? 0 : 1
  state_key = ($4 == "PENDING") ? 2 : ($4 == "SUSPENDED") ? 1 : 0
  printf "%d\t%d\t%d\t%d\t%s\n", part_key, state_key, -$5, $1, $0
}' <<<"$lines" | sort -t$'\t' -k1,1n -k2,2n -k3,3n -k4,4n | cut -f5- | awk -F'|' -v order="$order" -v first="$first" '
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
  # 優先度 1 以下は保留（scontrol top が下げられない。ADR-0030）、[ のある番号は配列の親ジョブ
  unmovable = (state == "PENDING" && (prio + 0 <= 1 || index(job, "[") > 0))
  if (!order) {
    printf "%s %s%s %s %s %s %s %s %s %s\n", job, state, unmovable ? "（動かせない）" : "", part, prio, class, limit, start, name, ref
    printf "  目的: %s\n", purpose
    printf "  連絡先: %s\n", contact
  }
  if (state != "PENDING" || part != "main" || unmovable) next
  if (class == "-") {
    unclassed = unclassed (unclassed == "" ? "" : ",") job
    last_unclassed = job
    next
  }
  # 区分の無いジョブが前にいれば、列を渡すと列のジョブが前に出るので、並べ直しが要る
  if (last_unclassed != "") unclassed_ahead = 1
  # 先に並ぶ、より下の区分のジョブのうち最も近いもの（無ければ、先に並ぶ区分の無いジョブ）
  found = 0
  for (k = count; k >= 1; k--) {
    if (rank[pend_class[k]] > rank[class]) {
      behind = behind (behind == "" ? "" : "、") sprintf("%s（%s）が %s（%s）の後ろ", job, class, pend_job[k], pend_class[k])
      found = 1
      break
    }
  }
  if (!found && last_unclassed != "") behind = behind (behind == "" ? "" : "、") sprintf("%s（%s）が区分の無い %s の後ろ", job, class, last_unclassed)
  count++
  pend_job[count] = job
  pend_class[count] = class
}
END {
  # 区分の順、同じ区分は今の順（--first のジョブは区分の先頭）に並べる（挿入ソート。待っているジョブは数本）
  for (i = 1; i <= count; i++) {
    idx[i] = i
    pos[i] = (pend_job[i] == first) ? 0 : i
  }
  for (i = 2; i <= count; i++) {
    v = idx[i]
    for (j = i - 1; j >= 1 && (rank[pend_class[idx[j]]] > rank[pend_class[v]] || (rank[pend_class[idx[j]]] == rank[pend_class[v]] && pos[idx[j]] > pos[v])); j--) idx[j + 1] = idx[j]
    idx[j + 1] = v
  }
  list = ""; now = ""
  for (i = 1; i <= count; i++) {
    list = list (i == 1 ? "" : ",") pend_job[idx[i]]
    now = now (i == 1 ? "" : ",") pend_job[i]
  }
  if (list == now && !unclassed_ahead) exit
  if (order) {
    print list
    exit
  }
  note = (unclassed == "") ? "" : sprintf("（区分の無い %s は後ろに回る）", unclassed)
  printf "ORDER: main: %sで待っている。scontrol top %s%s\n", behind, list, note
}'
