#!/usr/bin/env bash
# wait-step.sh — /delegate の担当が、ターンを終えずに待つ 1 歩
#
# Usage: wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...]
#        wait-step.sh --end <自分の worktree のパス>
#
# 担当は、待つ間このスクリプトをフォアグラウンドで続けて呼ぶ。ターンを終えると、そのたびに親に通知が届き親が応答する
# ので、終えずに待つ。1 回の呼び出しは約 4 分 30 秒で返るので、呼ぶたびの応答で担当のキャッシュ（5 分で切れる）が
# 保たれる。待つ間に届いた通知（/code-review の終わり、親の返事など）は、呼び出しが返った後に担当に渡る。
#
# 時計は担当ごとに 1 つで、worktree のパスで分ける（並行する担当どうしで、リポジトリやブランチの名前が重なっても取り合わ
# ない）。親に最後に途中の報告を送った時刻（初めは待ち始め）と、最後に呼んだ時刻を ${TMPDIR:-/tmp}/claude-wait-step/<鍵の
# ハッシュ> に残す。途中の報告から 55 分を超えたら REPORT を出して数え直す（担当は親に 1 通送って待ちを続ける。親のキャッシュの
# 1 時間が切れる前に、親に 1 回だけ応答させる。止めるかは親が決める）。前の呼び出しから 55 分を超えて空いていれば、終わった
# 待ち（--end の忘れ、中断した担当）の時計なので数え直す。待ちが済んだら --end で消す。DONE は、渡したファイルのうち終わった
# ものの 1 つを出す。ほかも待つなら、DONE の出たファイルを外して呼び直す。
#
# 出力と終了コード:
#   DONE: <ファイル>   0  渡した background の出力のどれかが終わった（Claude Code が最後の行に [exited with code N] か
#                         [killed] を書く）
#   WAIT: 途中の報告から <経過> 分             2  1 歩の時間がたった。もう一度呼ぶ
#   REPORT: 途中の報告から <経過> 分を超えた…  3  途中の報告から 55 分を超えた。親に途中の報告を送ってから、もう一度呼ぶ
#                                                 （数え直した）
#   使い方の誤り、渡した出力のファイルが無い（パスの写し損ね）、状態を書けないときは、理由を出して 1
#
# テストは WAIT_STEP_SECONDS（1 歩、既定 270）と WAIT_STEP_REPORT（途中の報告の間隔の秒、既定 3300）で時間を縮める

set -u

usage() {
  echo "Usage: wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...] | --end <自分の worktree のパス>" >&2
  exit 1
}

state_of() {
  # 鍵は正規化したパス（末尾の / や . を揃える）のハッシュにしてファイル名に使う（パスの記号や日本語でも重ならない）
  local key
  key=$(realpath -m -- "$1" | sha256sum)
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
reported="" last=""
[[ -s "$state" ]] && { read -r reported last <"$state" || true; }
# 前の呼び出しから報告の間隔を超えて空いていれば、終わった待ちの時計なので数え直す
if [[ -z "$reported" || -z "$last" ]] || (( now - last > report )); then
  reported=$now
fi
save() {
  if ! { mkdir -p "$(dirname "$state")" && echo "$reported $(date +%s)" >"$state"; } 2>/dev/null; then
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

# 1 秒ごとに、終わり、途中の報告の時刻、1 歩の時刻の順に見る（終わったものに途中の報告を出さない）。返る前に最後に
# 呼んだ時刻を残す
deadline=$(( now + step ))
while :; do
  if finished "$@"; then
    save
    exit 0
  fi
  t=$(date +%s)
  if (( t - reported >= report )); then
    elapsed=$(( (t - reported) / 60 ))
    reported=$t
    save
    echo "REPORT: 途中の報告から ${elapsed} 分を超えた。親に途中の報告を送ってから、もう一度呼ぶ"
    exit 3
  fi
  if (( t >= deadline )); then
    save
    echo "WAIT: 途中の報告から $(( (t - reported) / 60 )) 分"
    exit 2
  fi
  sleep 1
done
