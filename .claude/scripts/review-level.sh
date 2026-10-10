#!/usr/bin/env bash
# review-level.sh — ローカルの /code-review に渡すレベルと対象を決める
#
# Usage: review-level.sh [branch] [--base BASE]
#   branch  省略時は今のブランチ
#   BASE    省略時は既定ブランチ。origin 側がローカルより新しければ origin 側
#           （他の PR がマージされた直後はローカルが古く、比較元にすると
#           その PR の変更が差分に混ざる）。積み重なったブランチは土台のブランチを渡す
#
# 出力:
#   BASE: <比較元>
#   TARGET: <対象のブランチ>   /code-review に渡す。worktree で作業しているとき、引数なしの
#                              /code-review は呼び出し元の cwd の差分を見る。cwd が main の
#                              worktree だと差分が空になり、「指摘なし」と誤認する
#   FILES: <変更ファイル数>
#   LEVEL: medium | high       差分のファイルがすべて docs/ 配下なら medium、それ以外は high
#
# 差分が空、または refs を解決できないときは exit 1。
#
# 移動（rename）は旧パスも数える（--no-renames）。src/ から docs/ への移動は、
# コードを消す変更なので文書だけの変更として扱わない。
# docs/ の判定は pre-merge-check（is_docs_only）と decide-next.sh（docs_only）と同じ前方一致。

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

# 既定ブランチ。branch-topics.sh / worktree-add.sh と claude-hooks（git.rs の default_branch）も同じ順で探す
default_branch() {
  local ref
  if ref=$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null); then
    echo "$ref"
    return 0
  fi
  for ref in refs/remotes/origin/main refs/remotes/origin/master refs/heads/main refs/heads/master; do
    if git show-ref --verify --quiet "$ref"; then
      echo "${ref#refs/*/}"
      return 0
    fi
  done
  return 1
}

if [[ -z "$BASE" ]]; then
  if ! found=$(default_branch); then
    echo "ERROR: 既定ブランチが見つかりません。--base で比べる起点を指定してください" >&2
    exit 1
  fi
  name="${found#origin/}"
  BASE="$name"
  # origin 側がローカル以上に新しければ origin 側を使う。ローカルが先に進んでいる
  # （git-merge の流れ）ときや、ローカルが無いときの扱いは下の通り
  if git rev-parse --verify -q "refs/remotes/origin/${name}" >/dev/null; then
    if ! git rev-parse --verify -q "refs/heads/${name}" >/dev/null \
      || git merge-base --is-ancestor "$name" "origin/${name}" 2>/dev/null; then
      BASE="origin/${name}"
    fi
  fi
fi

if [[ -z "$TARGET" ]]; then
  TARGET=$(git branch --show-current)
  if [[ -z "$TARGET" ]]; then
    echo "ERROR: ブランチ名を渡してください（HEAD がブランチではありません）" >&2
    exit 1
  fi
fi

for ref in "$BASE" "$TARGET"; do
  if ! git rev-parse --verify -q "${ref}^{commit}" >/dev/null; then
    echo "ERROR: ${ref} を解決できません" >&2
    exit 1
  fi
done

files=$(git diff --no-renames --name-only "${BASE}...${TARGET}")
if [[ -z "$files" ]]; then
  echo "ERROR: ${BASE} と ${TARGET} の差分が空です。ブランチが別の worktree にあるなら、cwd ではなくブランチ名で渡してください" >&2
  exit 1
fi

if grep -qv '^docs/' <<<"$files"; then
  level=high
else
  level=medium
fi

echo "BASE: ${BASE}"
echo "TARGET: ${TARGET}"
echo "FILES: $(grep -c . <<<"$files")"
echo "LEVEL: ${level}"
