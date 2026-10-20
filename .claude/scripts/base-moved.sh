#!/usr/bin/env bash
# base-moved.sh — 比較元が分岐点から進み、ブランチと同じパスを触っていないかを知らせる
#
# Usage: base-moved.sh [branch] [--base BASE]
#   branch  省略時は今のブランチ
#   BASE    省略時は origin の既定ブランチ（origin/HEAD → origin/main → origin/master）。
#           積み重なったブランチは土台のブランチを渡す
#
# GitHub リモートがあれば、比べる前に fetch する（失敗したら WARN を出し、手元の追跡ブランチで比べる）。
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

git remote -v | grep -q 'github\.com' || exit 0

if ! git fetch -q; then
  echo "WARN: fetch に失敗しました。手元の追跡ブランチで比べます（古い可能性があります）" >&2
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

fork=$(git merge-base "$BASE" "$TARGET")
# マージコミットで入る PR は 1 件と数える（--first-parent）
moved=$(git rev-list --first-parent --count "${fork}..${BASE}")
((moved > 0)) || exit 0

# 移動（rename）は旧パスも数える（--no-renames）
common=$(comm -12 \
  <(git -c core.quotePath=false diff --no-renames --name-only "${fork}..${BASE}" | sort) \
  <(git -c core.quotePath=false diff --no-renames --name-only "${fork}..${TARGET}" | sort))
[[ -n "$common" ]] || exit 0

echo "BASE_MOVED: ${BASE} が分岐点から ${moved} commits 進んでいる（先頭 $(git rev-parse --short "$BASE")、分岐点 $(git rev-parse --short "$fork")）"
while IFS= read -r path; do echo "COMMON: ${path}"; done <<<"$common"
