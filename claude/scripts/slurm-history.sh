#!/usr/bin/env bash
# slurm-history.sh — jobcomp と sacct の記録から、ジョブの見積もり（--time）と実績とメモリの最大を比べる。--job は sacct から 1 本ずつ読む
#
# Usage: slurm-history.sh [--by-name] [--sacct] [FILE]
#        slurm-history.sh --job [--repo <本体のリポジトリ>] <ジョブ番号>...
#   FILE       jobcomp/filetxt のファイル。省略時は /var/log/slurm/job_completions
#   --sacct    sacct の記録も読む（FILE を省略したときは常に読む。FILE だけを渡したときは jobcomp だけで、sacct を呼ばない）
#   --by-name  ジョブ名ごとに集計する（省略時はジョブごと）
#   --job      ジョブごとに、状態、時間、排他か、コマンド、作業ディレクトリ、走った版（決められなければ理由）を
#              sacct の記録と git の reflog から出す。出力の形と版の決め方は lib/slurm-job.sh の先頭
#
# 出力（表のあとに 3 行）:
#   ジョブごと: JOBID NAME ESTIMATE ELAPSED RATIO MAX_RSS
#   ジョブ名ごと: NAME RUNS ESTIMATE ELAPSED RATIO MAX_RATIO MAX_RSS（ESTIMATE と ELAPSED は平均。
#   見積もりのあるジョブだけで求め、RUNS は見積もりのないジョブも数える。UNTIMED のジョブは数えない）
#   SKIPPED: <最後まで走っていないので表に入れなかった行数>
#   SHARED: <排他でないので表に入れなかった sacct のジョブの数>
#   UNTIMED: <割り込みを入れた後に終わった、timed でないので表に入れなかったジョブの数>
#
# 定義:
#   ELAPSED  EndTime - StartTime（jobcomp に Elapsed の欄は無い）
#   ESTIMATE TimeLimit の分を秒にしたもの。数値でない値（UNLIMITED など）は見積もりなし
#   RATIO    ELAPSED / ESTIMATE。1 を超えれば見積もりより長くかかった。見積もりなしは -
#            ジョブ名ごとの RATIO は ELAPSED の合計 / ESTIMATE の合計、MAX_RATIO は個々の比率の最大
# 対象: JobState=COMPLETED かつ NodeCnt が 1 以上の行。それ以外（開始前に CANCELLED、
#       FAILED、TIMEOUT など、最後まで走っていない実績）は SKIPPED に数えて表に入れない。
#       割り込みを入れた時刻（PREEMPT_SINCE）より後に終わったジョブは、割り込まれないパーティション timed のもの
#       （slurm-run.sh --timed）だけを表に入れ、ほかのパーティションのものは UNTIMED に数える。main のジョブは、止められ
#       なかったときも割り込まれうる条件で走った時間なので入れない。timed の前の urgent のジョブは、排他でないものと
#       見分けられないので入れない。それより前に終わったジョブは割り込みが無かったので
#       パーティションによらず表に入れる。
#
#   MAX_RSS  sacct のジョブのステップ（.batch など）の MaxRSS の最大。jobcomp は持たないので、sacct に無いジョブは -。
#            ジョブ名ごとは、ジョブの MAX_RSS の最大。M（MiB）か G（GiB）
# sacct: 2026-10-12（sacct の記録を始めた日）以降を読む。jobcomp にも sacct にもある JobId は sacct のほうを使う
#       （jobcomp の行を数えない）。SubmitLine の sbatch の直後の語が --cpus-per-task=N のジョブ（slurm-run.sh -c）は
#       排他でないので、時間が当てにならず、表に入れずに SHARED に数える。--exclusive か、読めない（slurm-run.sh を
#       通らない）ジョブは入れる。2026-10-12 より前の jobcomp は排他かを持たないので、名前の -shared で分ける。
#
# 行は `キー=値` の空白区切り。値に `=` や `,` を含む欄（Tres）があるので最初の `=` で分ける。
# `=` を含まない語は直前の値の続き（空白のあるジョブ名）として読む。
# 読めない行（JobId、JobState、TimeLimit、NodeCnt、時刻、PREEMPT_SINCE より後に終わった行の Partition の
# どれかが無いか壊れている）は、行番号を出して exit 1。ファイルが読めないときも exit 1。

set -euo pipefail

if [[ ${1:-} == --job ]]; then
  shift
  source "$(dirname "${BASH_SOURCE[0]}")/lib/slurm-job.sh"
  slurm_job_main "$@"
  exit
fi

BY_NAME=0
FILE=""
SACCT=0
while (($# > 0)); do
  case "$1" in
    --by-name)
      BY_NAME=1
      shift
      ;;
    --sacct)
      SACCT=1
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
[[ -n "$FILE" ]] || SACCT=1
FILE="${FILE:-/var/log/slurm/job_completions}"

if [[ ! -r "$FILE" ]]; then
  echo "ERROR: ファイルを読めません: $FILE" >&2
  exit 1
fi

# sacct の記録を始めた日（r995 に slurmdbd を置いた日。それより前のジョブは sacct に無い）。sacct の既定の範囲は今日だけなので、-S で渡す
SACCT_SINCE=2026-10-12

# sacct の記録を、awk が読む形（欄は \x1f 区切り、最後の欄を SubmitLine から排他かの語に替える）で $SACCT_FILE に書く。
# -X を付けず、ステップ（.batch）の MaxRSS も読む。SubmitLine は改行を含みうるので、続きの行（区切りの無い行）は捨てる
SACCT_FILE=$(mktemp)
trap 'rm -f "$SACCT_FILE"' EXIT
if ((SACCT)); then
  source "$(dirname "${BASH_SOURCE[0]}")/lib/slurm-job.sh"
  if ! raw=$(sacct -S "$SACCT_SINCE" -P -n --units=M --delimiter="$SLURM_JOB_SEP" \
    -o JobID,JobName,State,Partition,TimelimitRaw,Start,End,MaxRSS,SubmitLine); then # TimelimitRaw は分（sacct(1)。jobcomp の TimeLimit と同じ単位で、take が 60 倍して秒にする）
    echo "ERROR: sacct を読めませんでした" >&2
    exit 1
  fi
  while IFS= read -r line; do
    [[ $line == *"$SLURM_JOB_SEP"* ]] || continue
    IFS=$SLURM_JOB_SEP read -r id name state partition limit start end rss submitline <<<"$line"
    verdict=-
    if [[ $id != *.* ]]; then
      verdict=$(slurm_job_exclusive "$submitline")
      verdict=${verdict%% *}
    fi
    printf '%s\n' "$id$SLURM_JOB_SEP$name$SLURM_JOB_SEP$state$SLURM_JOB_SEP$partition$SLURM_JOB_SEP$limit$SLURM_JOB_SEP$start$SLURM_JOB_SEP$end$SLURM_JOB_SEP$rss$SLURM_JOB_SEP$verdict"
  done <<<"$raw" >"$SACCT_FILE"
fi

# 割り込み（パーティションの間の SUSPEND）を r995 に反映した時刻（2026-10-23 21:40:32 の nixos-rebuild switch）より少し前。
# これより前に終わったジョブは、どのパーティションでも止められていない
PREEMPT_SINCE=2026-10-23T21:40:00

# POSIX awk の機能だけで書く（mktime は gawk 拡張、正規表現の {n} は mawk の古い版が解さない）。時刻はどちらも同じ時刻帯
# なので、暦日から数えた通し秒の差で実績（経過時間）を求める
awk -v by_name="$BY_NAME" -v preempt_since="$PREEMPT_SINCE" -v sacct_file="$SACCT_FILE" '
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
# sacct の MaxRSS（--units=M で、数に K M G T の付いた値）を MiB で返す。空は -1
function mib(s,    u, v) {
  if (s == "") return -1
  if (s !~ /^[0-9.]+[KMGT]$/) return -2
  u = substr(s, length(s))
  v = substr(s, 1, length(s) - 1) + 0
  if (u == "K") return v / 1024
  if (u == "G") return v * 1024
  if (u == "T") return v * 1024 * 1024
  return v
}
function rss_text(m) {
  if (m < 0) return "-"
  if (m >= 1024) return sprintf("%.1fG", m / 1024)
  return sprintf("%.0fM", m)
}
function bad(why) {
  printf "ERROR: %s%d 行目を読めません（%s）\n", (FILENAME == sacct_file ? "sacct の " : ""), FNR, why > "/dev/stderr"
  failed = 1
  exit 1
}
# 1 件を表に加える（rss は MiB、無ければ -1）
function add(id, name, has_est, est, elapsed, ratio, rss) {
  if (by_name) {
    if (!(name in runs)) order[++n] = name
    runs[name]++
    if (rss > rss_max[name]) rss_max[name] = rss
    if (has_est) {
      est_sum[name] += est
      est_cnt[name]++
      el_sum[name] += elapsed
      if (est_cnt[name] == 1 || ratio > max_ratio[name]) max_ratio[name] = ratio
    }
  } else {
    rows[++n] = sprintf("%7s  %-24s %10s %10s %7s %8s", id, name, has_est ? hms(est) : "-", hms(elapsed), has_est ? sprintf("%.2f", ratio) : "-", rss_text(rss))
  }
}
# 対象かを決めて表に加える。sacct のジョブは、ステップの MaxRSS が後の行に来るので、END で加える
function take(id, name, state, nodes, limit, st, en, partition,    start, end, elapsed, has_est, est) {
  # 対象外の行は時刻を読まない（開始前の CANCELLED は StartTime が壊れうる）
  if (state != "COMPLETED" || nodes + 0 < 1) {
    skipped++
    return
  }
  start = epoch(st)
  end = epoch(en)
  if (start < 0) bad("StartTime がありません")
  if (end < 0) bad("EndTime がありません")
  if (end < start) bad("EndTime が StartTime より前です")
  # 時刻は同じ書式（YYYY-MM-DDTHH:MM:SS）なので文字列で比べる
  if (en >= preempt_since) {
    if (partition == "") bad("Partition がありません")
    if (partition != "timed") {
      untimed++
      return
    }
  }
  elapsed = end - start
  has_est = (limit ~ /^[0-9]+$/ && limit + 0 > 0)
  est = has_est ? limit * 60 : 0
  if (FILENAME == sacct_file) {
    pend[++np] = id
    pend_name[np] = name
    pend_has_est[np] = has_est
    pend_est[np] = est
    pend_elapsed[np] = elapsed
  } else {
    add(id, name, has_est, est, elapsed, has_est ? elapsed / est : -1, -1)
  }
}
FILENAME == sacct_file {
  c = split($0, f, "\037")
  if (c != 9) bad("欄の数が違います")
  id = f[1]
  if (index(id, ".") > 0) {
    # ステップ（.batch など）の行。MaxRSS はジョブの行には出ない
    m = mib(f[8])
    if (m == -2) bad("MaxRSS を読めません")
    base = substr(id, 1, index(id, ".") - 1)
    if (!(base in rss) || m > rss[base]) rss[base] = m
    next
  }
  sacct_ids[id] = 1
  if (f[9] == "no") {
    shared++
    next
  }
  take(id, f[2], f[3], 1, f[5], f[6], f[7], f[4])
  next
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
  if (!("JobId" in kv)) bad("JobId がありません")
  # sacct にもあるジョブは sacct のほうを使う（MaxRSS と排他かを持つ）
  if (kv["JobId"] in sacct_ids) next
  if (!("JobState" in kv)) bad("JobState がありません")
  if (!("TimeLimit" in kv)) bad("TimeLimit がありません")
  if (kv["NodeCnt"] !~ /^[0-9]+$/) bad("NodeCnt が数値ではありません")
  # 割り込みの後に終わった行の Partition は、take が読む（無ければ bad）
  take(kv["JobId"], kv["Name"], kv["JobState"], kv["NodeCnt"], kv["TimeLimit"], kv["StartTime"], kv["EndTime"], ("Partition" in kv) ? kv["Partition"] : "")
}
END {
  if (failed) exit 1
  for (i = 1; i <= np; i++) {
    r = (pend[i] in rss) ? rss[pend[i]] : -1
    add(pend[i], pend_name[i], pend_has_est[i], pend_est[i], pend_elapsed[i], pend_has_est[i] ? pend_elapsed[i] / pend_est[i] : -1, r)
  }
  if (by_name) {
    printf "%-24s %5s %10s %10s %7s %9s %8s\n", "NAME", "RUNS", "ESTIMATE", "ELAPSED", "RATIO", "MAX_RATIO", "MAX_RSS"
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
      printf "%-24s %5d %10s %10s %7s %9s %8s\n", nm, runs[nm], e, a, r, mr, rss_text((nm in rss_max) ? rss_max[nm] : -1)
    }
  } else {
    printf "%7s  %-24s %10s %10s %7s %8s\n", "JOBID", "NAME", "ESTIMATE", "ELAPSED", "RATIO", "MAX_RSS"
    for (i = 1; i <= n; i++) print rows[i]
  }
  printf "SKIPPED: %d\n", skipped + 0
  printf "UNTIMED: %d\n", untimed + 0
  printf "SHARED: %d\n", shared + 0
}
' "$SACCT_FILE" "$FILE"
