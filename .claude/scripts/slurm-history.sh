#!/usr/bin/env bash
# slurm-history.sh — jobcomp の記録から、ジョブの見積もり（--time）と実績を比べる
#
# Usage: slurm-history.sh [--by-name] [FILE]
#   FILE       jobcomp/filetxt のファイル。省略時は /var/log/slurm/job_completions
#   --by-name  ジョブ名ごとに集計する（省略時はジョブごと）
#
# 出力（表のあとに 1 行）:
#   ジョブごと: JOBID NAME ESTIMATE ELAPSED RATIO
#   ジョブ名ごと: NAME RUNS ESTIMATE ELAPSED RATIO MAX_RATIO（ESTIMATE と ELAPSED は平均。
#   見積もりのあるジョブだけで求め、RUNS は見積もりのないジョブも数える）
#   SKIPPED: <表に入れなかった行数>
#
# 定義（ADR-0019）:
#   ELAPSED  EndTime - StartTime（jobcomp に Elapsed の欄は無い）
#   ESTIMATE TimeLimit の分を秒にしたもの。数値でない値（UNLIMITED など）は見積もりなし
#   RATIO    ELAPSED / ESTIMATE。1 を超えれば見積もりより長くかかった。見積もりなしは -
#            ジョブ名ごとの RATIO は ELAPSED の合計 / ESTIMATE の合計、MAX_RATIO は個々の比率の最大
# 対象: JobState=COMPLETED かつ NodeCnt が 1 以上の行。それ以外（開始前に CANCELLED、
#       FAILED、TIMEOUT など、最後まで走っていない実績）は SKIPPED に数えて表に入れない。
#
# 行は `キー=値` の空白区切り。値に `=` や `,` を含む欄（Tres）があるので最初の `=` で分ける。
# `=` を含まない語は直前の値の続き（空白のあるジョブ名）として読む。
# 読めない行（JobId、JobState、TimeLimit、NodeCnt、時刻のどれかが無いか壊れている）は、
# 行番号を出して exit 1。ファイルが読めないときも exit 1。

set -euo pipefail

BY_NAME=0
FILE=""
while (($# > 0)); do
  case "$1" in
    --by-name)
      BY_NAME=1
      shift
      ;;
    -*)
      echo "ERROR: 不明なオプション: $1" >&2
      exit 1
      ;;
    *)
      if [[ -n "$FILE" ]]; then
        echo "ERROR: ファイルは 1 つだけ渡せます: $FILE と $1" >&2
        exit 1
      fi
      FILE="$1"
      shift
      ;;
  esac
done
FILE="${FILE:-/var/log/slurm/job_completions}"

if [[ ! -r "$FILE" ]]; then
  echo "ERROR: ファイルを読めません: $FILE" >&2
  exit 1
fi

# POSIX awk の機能だけで書く（mktime は gawk 拡張、正規表現の {n} は mawk の古い版が解さない）。時刻はどちらも同じ時刻帯の
# 壁時計なので、暦日から数えた通し秒の差で実績を求める
awk -v by_name="$BY_NAME" '
function days(y, m, d,    a) {
  # 暦日から通し日数への変換（Howard Hinnant の days_from_civil）
  if (m <= 2) y--
  a = int(y / 400)
  yoe = y - a * 400
  doy = int((153 * (m + (m > 2 ? -3 : 9)) + 2) / 5) + d - 1
  doe = yoe * 365 + int(yoe / 4) - int(yoe / 100) + doy
  return a * 146097 + doe
}
function epoch(s,    p) {
  if (s !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]$/) return -1
  split(s, p, /[-T:]/)
  if (p[2] < 1 || p[2] > 12 || p[3] < 1 || p[3] > 31 || p[4] > 23 || p[5] > 59 || p[6] > 59) return -1
  return days(p[1] + 0, p[2] + 0, p[3] + 0) * 86400 + p[4] * 3600 + p[5] * 60 + p[6]
}
function hms(sec,    s) {
  s = int(sec + 0.5)
  return sprintf("%d:%02d:%02d", int(s / 3600), int(s % 3600 / 60), s % 60)
}
function bad(n, why) {
  printf "ERROR: %d 行目を読めません（%s）\n", n, why > "/dev/stderr"
  failed = 1
  exit 1
}
NF == 0 { next }
{
  delete kv
  key = ""
  for (i = 1; i <= NF; i++) {
    eq = index($i, "=")
    if (eq > 0) {
      key = substr($i, 1, eq - 1)
      kv[key] = substr($i, eq + 1)
    } else if (key != "") {
      kv[key] = kv[key] " " $i
    }
  }
  if (!("JobId" in kv)) bad(NR, "JobId がありません")
  if (!("JobState" in kv)) bad(NR, "JobState がありません")
  if (!("TimeLimit" in kv)) bad(NR, "TimeLimit がありません")
  if (kv["NodeCnt"] !~ /^[0-9]+$/) bad(NR, "NodeCnt が数値ではありません")

  # 対象外の行は時刻を読まない（開始前の CANCELLED は StartTime が壊れうる）
  if (kv["JobState"] != "COMPLETED" || kv["NodeCnt"] + 0 < 1) {
    skipped++
    next
  }
  start = epoch(kv["StartTime"])
  end = epoch(kv["EndTime"])
  if (start < 0) bad(NR, "StartTime がありません")
  if (end < 0) bad(NR, "EndTime がありません")
  if (end < start) bad(NR, "EndTime が StartTime より前です")
  elapsed = end - start
  has_est = (kv["TimeLimit"] ~ /^[0-9]+$/ && kv["TimeLimit"] + 0 > 0)
  est = has_est ? kv["TimeLimit"] * 60 : 0
  ratio = has_est ? elapsed / est : -1
  name = kv["Name"]

  if (by_name) {
    if (!(name in runs)) order[++n] = name
    runs[name]++
    if (has_est) {
      est_sum[name] += est
      est_cnt[name]++
      el_sum[name] += elapsed
      if (est_cnt[name] == 1 || ratio > max_ratio[name]) max_ratio[name] = ratio
    }
  } else {
    rows[++n] = sprintf("%7s  %-24s %10s %10s %7s", kv["JobId"], name, has_est ? hms(est) : "-", hms(elapsed), has_est ? sprintf("%.2f", ratio) : "-")
  }
}
END {
  if (failed) exit 1
  if (by_name) {
    printf "%-24s %5s %10s %10s %7s %9s\n", "NAME", "RUNS", "ESTIMATE", "ELAPSED", "RATIO", "MAX_RATIO"
    for (i = 1; i <= n; i++) {
      nm = order[i]
      if (est_cnt[nm] > 0) {
        e = hms(est_sum[nm] / est_cnt[nm])
        a = hms(el_sum[nm] / est_cnt[nm])
        r = sprintf("%.2f", el_sum[nm] / est_sum[nm])
        mr = sprintf("%.2f", max_ratio[nm])
      } else {
        e = "-"; a = "-"; r = "-"; mr = "-"
      }
      printf "%-24s %5d %10s %10s %7s %9s\n", nm, runs[nm], e, a, r, mr
    }
  } else {
    printf "%7s  %-24s %10s %10s %7s\n", "JOBID", "NAME", "ESTIMATE", "ELAPSED", "RATIO"
    for (i = 1; i <= n; i++) print rows[i]
  }
  printf "SKIPPED: %d\n", skipped + 0
}
' "$FILE"
