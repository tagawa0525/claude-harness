#!/usr/bin/env bash
# base-moved.sh — 比較元が分岐点から進み、ブランチと同じパスを触っていないかを知らせる
#
# Usage: base-moved.sh [branch] [--base BASE]
#   branch  省略時は今のブランチ
#   BASE    省略時は origin の既定ブランチ（origin/HEAD → origin/main → origin/master）。
#           積み重なったブランチは土台を origin/<土台のブランチ> で渡す（fetch が更新するのは
#           追跡ブランチだけで、ローカルのブランチは古いまま比べることになる）
#
# --base を渡されたときは、どの clone でも比べ、--base が追跡ブランチならそのリモートを fetch してから比べる。
# 渡されないときは、GitHub リモートと origin が揃うときだけ、origin を fetch して origin の既定ブランチと比べ、
# 揃わなければ何もしない（リモートの無いリポジトリでは --base main を渡す）。fetch に失敗したら WARN を出し、
# 手元の追跡ブランチで比べる。
# WARN が出たときは比較が済んでいない（出力が空でも「進みなし」ではない）。
# 他のセッションが同じファイルを main に入れていると、マージ直前の rebase まで気付けない。
# 知らせるだけで失敗にはしない（同じパスを触っても、衝突するとは限らない）。
#
# 出力（BASE が分岐点から進み、進んだ変更とブランチの変更が同じパスを触るときだけ。それ以外は何も出さない）:
#   BASE_MOVED: <BASE> が分岐点から <N> commits 進んでいる（先頭 <hash>、分岐点 <hash>）
#   COMMON: <両方が触ったパス>   パスごとに 1 行

set -euo pipefail

BASE=""
TARGET=""
while (($# > 0)); do
  case "$1" in
    --base)
      BASE="${2:?--base には比較元が必要です}"
      shift 2
      ;;
    -*)
      echo "ERROR: 不明なオプション: $1" >&2
      exit 1
      ;;
    *)
      TARGET="$1"
      shift
      ;;
  esac
done

# パイプにしない（pipefail 下で grep -q が先に終わると SIGPIPE で失敗して、GitHub リモートがあってもスキップする）
remotes=$(git remote -v)
fetch_or_warn() {
  if ! git fetch -q "$1"; then
    echo "WARN: fetch に失敗しました。比較は未確認です（手元の追跡ブランチは古い可能性があります）" >&2
  fi
}
# BASE が追跡ブランチ（<リモート>/...、remotes/<リモート>/...、refs/remotes/<リモート>/...）なら、そのリモートの名前。
# まだ fetch していない追跡ブランチも fetch してから比べるので、参照を解かずに文字列で、登録されたリモートのうち
# 前方一致する最長の名前を取る（リモート名に / を含みうる）
remote_of() {
  local b="$1" name best=""
  # ローカルのブランチなら fetch しない（feat/x のような名前が、同名のリモートの追跡ブランチに見える）
  git show-ref --verify --quiet "refs/heads/$b" && return 0
  b="${b#refs/}"
  b="${b#remotes/}"
  while IFS= read -r name; do
    if [[ "$b" == "$name"/* && ${#name} -gt ${#best} ]]; then best="$name"; fi
  done < <(git remote)
  if [[ -z "$best" && "$1" == *remotes/* ]]; then
    echo "WARN: ${1} のリモートが見つかりません。比較は未確認です（手元の追跡ブランチは古い可能性があります）" >&2
  fi
  echo "$best"
}
if [[ -n "$BASE" ]]; then
  base_remote=$(remote_of "$BASE")
  [[ -z "$base_remote" ]] || fetch_or_warn "$base_remote"
elif grep -q 'github\.com' <<<"$remotes" && git remote get-url origin >/dev/null 2>&1; then
  # origin を明示する。引数なしの fetch は今のブランチの上流のリモートへ行き、
  # 上流が別のリモートだと、BASE の origin/* が更新されないまま古い比較になる
  fetch_or_warn origin
else
  # 比べる先の既定は GitHub の origin の既定ブランチなので、GitHub リモートか origin が無ければ決まらない
  exit 0
fi

if [[ -z "$BASE" ]]; then
  if ref=$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null); then
    BASE="$ref"
  else
    for ref in origin/main origin/master; do
      if git rev-parse --verify -q "refs/remotes/$ref" >/dev/null; then
        BASE="$ref"
        break
      fi
    done
  fi
  [[ -n "$BASE" ]] || exit 0
fi

TARGET="${TARGET:-$(git branch --show-current)}"
[[ -n "$TARGET" ]] || { echo "ERROR: ブランチ名を渡してください（HEAD がブランチではありません）" >&2; exit 1; }

for ref in "$BASE" "$TARGET"; do
  git rev-parse --verify -q "${ref}^{commit}" >/dev/null || { echo "ERROR: ${ref} を解決できません" >&2; exit 1; }
done

# merge-base は共通の祖先が無いとき 1、それ以外の失敗（壊れた repo など）はほかの終了コードを返す
# stderr は失敗のときだけ使う（成功時の警告を分岐点の値に混ぜない）
rc=0
mb_err=$(mktemp)
trap 'rm -f "$mb_err"' EXIT
fork=$(git merge-base "$BASE" "$TARGET" 2>"$mb_err") || rc=$?
if ((rc == 1)); then
  echo "WARN: ${BASE} と ${TARGET} の分岐点が見つかりません（履歴が共通でない、または shallow clone）。比較は未確認です" >&2
  exit 0
elif ((rc != 0)); then
  echo "WARN: git merge-base が失敗しました（終了コード ${rc}）。比較は未確認です: $(<"$mb_err")" >&2
  exit 0
fi
# マージコミットで入る PR は 1 件と数える（--first-parent）
moved=$(git rev-list --first-parent --count "${fork}..${BASE}")
((moved > 0)) || exit 0

# 移動（rename）は旧パスも数える（--no-renames）。diff は変数に受けて終了コードを見る
# （プロセス置換だと失敗が見えず、空の出力＝進みなしと区別できない）
if ! base_files=$(git -c core.quotePath=false diff --no-renames --name-only "${fork}..${BASE}") \
  || ! branch_files=$(git -c core.quotePath=false diff --no-renames --name-only "${fork}..${TARGET}"); then
  echo "WARN: git diff が失敗しました。比較は未確認です" >&2
  exit 0
fi
common=$(comm -12 <(sort <<<"$base_files") <(sort <<<"$branch_files"))
[[ -n "$common" ]] || exit 0

echo "BASE_MOVED: ${BASE} が分岐点から ${moved} commits 進んでいる（先頭 $(git rev-parse --short "$BASE")、分岐点 $(git rev-parse --short "$fork")）"
while IFS= read -r path; do echo "COMMON: ${path}"; done <<<"$common"
