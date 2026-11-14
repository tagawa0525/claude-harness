#!/usr/bin/env bash
# wait-step.sh — /delegate の担当が、ターンを終えずに待つ 1 歩
#
# Usage: wait-step.sh <待ちの名前> [<background の出力のファイル>...]
#        wait-step.sh --end <待ちの名前>
#
# 担当は、待つ間このスクリプトをフォアグラウンドで続けて呼ぶ。ターンを終えると、そのたびに親に通知が届き親が応答する
# ので、終えずに待つ。1 回の呼び出しは約 4 分 30 秒で返るので、呼ぶたびの応答で担当のキャッシュ（5 分で切れる）が
# 保たれる。待つ間に届いた通知（/code-review の終わりなど）は、呼び出しが返った後に担当に渡る。
#
# 待ちの名前ごとに、最初に呼んだ時刻を ${TMPDIR:-/tmp}/claude-wait-step/<worktree>/<名前> に残し、そこからの経過で
# 打ち切る（worktree は cwd の git のトップ。並行する担当どうしで、同じ名前の待ちの時計を取り合わない）。55 分で打ち切る
# のは、親のキャッシュ（1 時間）が切れる前に報告を届けるため。待ちが済んだら --end で消す。STOP を出したときは自分で消す
# （次に同じ名前で待つときに、古い時刻のまま打ち切らない）。
# DONE は、渡したファイルのうち終わったものの 1 つを出す。ほかのジョブも待つなら、DONE の出たファイルを外して呼び直す。
#
# 出力と終了コード:
#   DONE: <ファイル>   0  渡した background の出力のどれかが終わった（Claude Code が最後の行に [exited with code N] を書く）
#   WAIT: <経過> 分    2  1 歩の時間がたった。もう一度呼ぶ
#   STOP: ...         3  待ち始めから打ち切りの時間を超えた。待たずに報告して止まる
#
# テストは WAIT_STEP_SECONDS（1 歩、既定 270）と WAIT_STEP_LIMIT（打ち切り、既定 3300）で時間を縮める

set -u

usage() {
  echo "Usage: wait-step.sh <待ちの名前> [<background の出力のファイル>...] | --end <待ちの名前>" >&2
  exit 1
}

state_of() {
  # 名前と worktree はファイル名に使うので、英数字と - _ . のほかは _ にする
  local name=${1//[^A-Za-z0-9._-]/_} top
  top=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
  top=${top//[^A-Za-z0-9._-]/_}
  printf '%s/claude-wait-step/%s/%s' "${TMPDIR:-/tmp}" "$top" "$name"
}

[[ $# -ge 1 && -n "$1" ]] || usage
if [[ "$1" == --end ]]; then
  [[ $# -eq 2 && -n "$2" ]] || usage
  rm -f "$(state_of "$2")"
  exit 0
fi

state=$(state_of "$1")
shift
step=${WAIT_STEP_SECONDS:-270}
limit=${WAIT_STEP_LIMIT:-3300}
now=$(date +%s)
mkdir -p "$(dirname "$state")"
[[ -s "$state" ]] || echo "$now" >"$state"
started=$(<"$state")

finished() {
  local f
  for f in "$@"; do
    # 最後の行だけを見る（途中の行の同じ文字列で誤らず、大きな出力を毎秒読み直さない）
    if [[ -f "$f" ]] && tail -n 1 "$f" | grep -q '^\[exited with code'; then
      echo "DONE: $f"
      return 0
    fi
  done
  return 1
}

stop_if_over() {
  local now
  now=$(date +%s)
  if (( now - started >= limit )); then
    rm -f "$state"
    echo "STOP: 待ち始めから $(( (now - started) / 60 )) 分。待つのをやめて報告して止まる"
    exit 3
  fi
}

stop_if_over
deadline=$(( now + step ))
# 打ち切りの時刻を越えて待たない
(( deadline > started + limit )) && deadline=$(( started + limit ))
while :; do
  finished "$@" && exit 0
  (( $(date +%s) >= deadline )) && break
  sleep 1
done
stop_if_over
echo "WAIT: $(( ($(date +%s) - started) / 60 )) 分"
exit 2
