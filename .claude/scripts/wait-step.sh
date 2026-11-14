#!/usr/bin/env bash
# wait-step.sh — /delegate の担当が、ターンを終えずに待つ 1 歩
#
# Usage: wait-step.sh <待ちの名前> [<background の出力のファイル>...]
#        wait-step.sh --end <待ちの名前>
#
# 担当は、待つ間このスクリプトをフォアグラウンドで続けて呼ぶ。ターンを終えると、そのたびに親に通知が届き親が応答する
# ので、終えずに待つ。1 回の呼び出しは約 4 分 30 秒で返るので、呼ぶたびの応答で担当のキャッシュ（5 分で切れる）が
# 保たれる。待つ間に届いた通知（/code-review の終わり、親の返事など）は、呼び出しが返った後に担当に渡る。
#
# 待ちの名前ごとに、時計の始まりと最後に呼んだ時刻を ${TMPDIR:-/tmp}/claude-wait-step/<名前のハッシュ> に残す。並行する
# 担当どうしで時計を取り合わないよう、名前にブランチ名を入れる（`<ブランチ>-merge`）。時計が 55 分を超えたら REPORT を
# 出して時計を数え直す。担当は親に途中の報告を 1 通送り、待ちを続ける（親のキャッシュの 1 時間が切れる前に、親に 1 回だけ
# 応答させる）。前の呼び出しから 1 歩の 2 倍より長く空いたとき（消し忘れた前の待ち）も数え直す。待ちが済んだら --end で消す。
# DONE は、渡したファイルのうち終わったものの 1 つを出す。ほかのジョブも待つなら、DONE の出たファイルを外して呼び直す。
#
# 出力と終了コード:
#   DONE: <ファイル>   0  渡した background の出力のどれかが終わった（Claude Code が最後の行に [exited with code N] を書く）
#   WAIT: <経過> 分    2  1 歩の時間がたった。もう一度呼ぶ
#   REPORT: <経過> 分  3  時計が 55 分を超えた。親に途中の報告を送ってから、もう一度呼ぶ（時計は数え直した）
#   使い方の誤り、渡した出力のファイルが無い（パスの写し損ね）は、理由を出して 1
#
# テストは WAIT_STEP_SECONDS（1 歩、既定 270）と WAIT_STEP_REPORT（途中の報告の間隔、既定 3300）で時間を縮める

set -u

usage() {
  echo "Usage: wait-step.sh <待ちの名前> [<background の出力のファイル>...] | --end <待ちの名前>" >&2
  exit 1
}

state_of() {
  # 名前はハッシュにしてファイル名に使う（日本語や記号の名前でも重ならない）
  local key
  key=$(printf '%s' "$1" | sha256sum)
  printf '%s/claude-wait-step/%s' "${TMPDIR:-/tmp}" "${key%% *}"
}

[[ $# -ge 1 && -n "$1" ]] || usage
if [[ "$1" == --end ]]; then
  [[ $# -eq 2 && -n "$2" ]] || usage
  rm -f "$(state_of "$2")"
  exit 0
fi

state=$(state_of "$1")
shift
for f in "$@"; do
  [[ -f "$f" ]] || { echo "ERROR: 出力のファイルがありません: $f" >&2; exit 1; }
done
step=${WAIT_STEP_SECONDS:-270}
report=${WAIT_STEP_REPORT:-3300}
now=$(date +%s)
mkdir -p "$(dirname "$state")"
started="" last=""
[[ -s "$state" ]] && { read -r started last <"$state" || true; }
# 前の呼び出しから 1 歩の 2 倍より長く空いていれば、続いている待ちでないので数え直す
if [[ -z "$started" || -z "$last" ]] || (( now - last > 2 * step )); then
  started=$now
fi
echo "$started $now" >"$state"

finished() {
  local f
  for f in "$@"; do
    # 最後の行だけを見る（途中の行の同じ文字列で誤らず、大きな出力を毎秒読み直さない）
    if tail -n 1 "$f" | grep -q '^\[exited with code'; then
      echo "DONE: $f"
      return 0
    fi
  done
  return 1
}

# 時計が報告の間隔を超えていれば、REPORT を出して時計を数え直す
report_if_due() {
  local t
  t=$(date +%s)
  if (( t - started >= report )); then
    echo "$t $t" >"$state"
    echo "REPORT: 待ちが $(( (t - started) / 60 )) 分を超えた。親に途中の報告を送ってから、もう一度呼ぶ"
    exit 3
  fi
}

report_if_due
deadline=$(( now + step ))
# 報告の時刻を越えて待たない
(( deadline > started + report )) && deadline=$(( started + report ))
while :; do
  finished "$@" && exit 0
  (( $(date +%s) >= deadline )) && break
  sleep 1
done
report_if_due
echo "$started $(date +%s)" >"$state"
echo "WAIT: $(( ($(date +%s) - started) / 60 )) 分"
exit 2
