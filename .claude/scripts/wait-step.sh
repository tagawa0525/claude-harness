#!/usr/bin/env bash
# wait-step.sh — /delegate の担当が、ターンを終えずに待つ 1 歩
#
# Usage: wait-step.sh <ブランチ名> [--limit <分>] [<background の出力のファイル>...]
#        wait-step.sh --end <ブランチ名>
#
# 担当は、待つ間このスクリプトをフォアグラウンドで続けて呼ぶ。ターンを終えると、そのたびに親に通知が届き親が応答する
# ので、終えずに待つ。1 回の呼び出しは約 4 分 30 秒で返るので、呼ぶたびの応答で担当のキャッシュ（5 分で切れる）が
# 保たれる。待つ間に届いた通知（/code-review の終わり、親の返事など）は、呼び出しが返った後に担当に渡る。
#
# 時計は担当ごとに 1 つ（ブランチ名で分ける。並行する担当どうしで取り合わない）で、待ち始めの時刻と、親に最後に途中の
# 報告を送った時刻を ${TMPDIR:-/tmp}/claude-wait-step/<名前のハッシュ> に残す。途中の報告から 55 分を超えたら REPORT を
# 出して数え直す（担当は親に 1 通送って待ちを続ける。親のキャッシュの 1 時間が切れる前に、親に 1 回だけ応答させる）。
# --limit は、止まりうる待ち（通知の来ない止まった /code-review）を、待ち始めからその分で STOP にする。待ちが済んだら
# --end で消す。DONE は、渡したファイルのうち終わったものの 1 つを出す。ほかも待つなら、DONE の出たファイルを外して呼び直す。
#
# 出力と終了コード:
#   DONE: <ファイル>   0  渡した background の出力のどれかが終わった（Claude Code が最後の行に [exited with code N] か
#                         [killed] を書く）
#   WAIT: <経過> 分    2  1 歩の時間がたった。もう一度呼ぶ
#   REPORT: <経過> 分  3  途中の報告から 55 分を超えた。親に途中の報告を送ってから、もう一度呼ぶ（数え直した）
#   STOP: <経過> 分    4  待ち始めから --limit の分を超えた。待つのをやめる
#   使い方の誤り、渡した出力のファイルが無い（パスの写し損ね）、状態を書けないときは、理由を出して 1
#
# テストは WAIT_STEP_SECONDS（1 歩、既定 270）、WAIT_STEP_REPORT（途中の報告の間隔の秒、既定 3300）、
# WAIT_STEP_MINUTE（--limit の 1 分の秒、既定 60）で時間を縮める

set -u

usage() {
  echo "Usage: wait-step.sh <ブランチ名> [--limit <分>] [<background の出力のファイル>...] | --end <ブランチ名>" >&2
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
limit=""
if [[ "${1:-}" == --limit ]]; then
  [[ "${2:-}" =~ ^[0-9]+$ ]] || usage
  limit=$(( $2 * ${WAIT_STEP_MINUTE:-60} ))
  shift 2
fi
for f in "$@"; do
  [[ -f "$f" ]] || { echo "ERROR: 出力のファイルがありません: $f" >&2; exit 1; }
done
step=${WAIT_STEP_SECONDS:-270}
report=${WAIT_STEP_REPORT:-3300}
now=$(date +%s)
started="" reported=""
[[ -s "$state" ]] && { read -r started reported <"$state" || true; }
[[ -n "$started" && -n "$reported" ]] || { started=$now; reported=$now; }
save() {
  if ! { mkdir -p "$(dirname "$state")" && echo "$started $reported" >"$state"; } 2>/dev/null; then
    echo "ERROR: 待ちの状態を書けません: $state" >&2
    exit 1
  fi
}
save

finished() {
  local f
  for f in "$@"; do
    # 最後の行だけを見る（途中の行の同じ文字列で誤らず、大きな出力を毎秒読み直さない）
    if tail -n 1 "$f" | grep -qE '^\[(exited with code|killed)'; then
      echo "DONE: $f"
      return 0
    fi
  done
  return 1
}

# 終わりを先に見て、済んだものに途中の報告や打ち切りを出さない
check() {
  local t
  finished "$@" && exit 0
  t=$(date +%s)
  if [[ -n "$limit" ]] && (( t - started >= limit )); then
    echo "STOP: 待ち始めから $(( (t - started) / 60 )) 分。待つのをやめる"
    exit 4
  fi
  if (( t - reported >= report )); then
    reported=$t
    save
    echo "REPORT: 途中の報告から $(( report / 60 )) 分を超えた。親に途中の報告を送ってから、もう一度呼ぶ"
    exit 3
  fi
}

check "$@"
deadline=$(( now + step ))
# 途中の報告と打ち切りの時刻を越えて待たない
(( deadline > reported + report )) && deadline=$(( reported + report ))
[[ -n "$limit" ]] && (( deadline > started + limit )) && deadline=$(( started + limit ))
while (( $(date +%s) < deadline )); do
  finished "$@" && exit 0
  sleep 1
done
check "$@"
echo "WAIT: $(( ($(date +%s) - started) / 60 )) 分"
exit 2
