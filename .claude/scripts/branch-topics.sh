#!/usr/bin/env bash
# branch-topics.sh — ブランチが含むトピックを要約する（1 ブランチ 1 トピックの確認用）
#
# Usage: branch-topics.sh [base [head]]
#
# base を省くと既定ブランチ（origin/HEAD → origin/main → origin/master → main → master の
# 最初にあるもの。worktree-add.sh と同じ）と比べる。変更は merge-base から HEAD まで
# （base...HEAD）を見るので、分岐した後に base が進んでいても混ざらない。head を渡すと HEAD の代わりに
# そのコミットを要約する（チェックアウトしていない PR のブランチを見るとき。例: origin/main origin/feat/x）。
#
# ADR と issue は docs/adr/・docs/issues/ の直下にある NNNN-name.md（4 桁の番号とハイフンで始まる）。
# README などは含めない。status は前付けの `status:` の値（design-records status で読む。前付けの形が
# 崩れていれば失敗する）。
# ディレクトリが無いリポジトリでは該当の行を出さない。
#
# 出力（行がないものは省く）:
#   BASE: <ref>
#   HEAD: <ref>（要約したコミット。head を省けば HEAD）
#   COMMITS: <n>
#   TYPES: <type>=<n> ...（件数の多い順、同数は名前順。Conventional Commits の型がない件名は other）
#   ADR_NEW: <path> (<status>)            … 1 件 1 行
#   ADR_STATUS: <path> <old> -> <new>      … 既存の ADR の status の変化
#   ISSUE_NEW: <path> (<status>)
#   ISSUE_STATUS: <path> <old> -> <new>
#   DIRS: <dir> ...（変更したファイルのディレクトリの先頭 2 段。リポジトリ直下は .）

set -euo pipefail

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "ERROR: git リポジトリではありません" >&2
  exit 1
fi

# 既定ブランチ。worktree-add.sh と claude-hooks（git.rs の default_branch）も同じ順で探す
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

case $# in
  0)
    if ! BASE=$(default_branch); then
      echo "ERROR: 既定ブランチが見つかりません。比べる起点を引数で指定してください" >&2
      exit 1
    fi
    ;;
  1 | 2) BASE="$1" ;;
  *)
    echo "Usage: $0 [base [head]]" >&2
    exit 1
    ;;
esac

TIP="${2:-HEAD}"
for ref in "$BASE" "$TIP"; do
  if ! git rev-parse --verify --quiet "$ref^{commit}" >/dev/null; then
    echo "ERROR: コミットが見つかりません: $ref" >&2
    exit 1
  fi
done
FORK=$(git merge-base "$BASE" "$TIP")

echo "BASE: $BASE"
echo "HEAD: $TIP"
echo "COMMITS: $(git rev-list --count "$BASE..$TIP")"

TYPES=$(git log --format=%s "$BASE..$TIP" \
  | sed -E 's/^([a-z]+)(\([^)]*\))?!?: .*/\1/; t; s/.*/other/' \
  | sort | uniq -c | sort -k1,1nr -k2,2 \
  | awk '{ printf "%s%s=%s", (NR > 1 ? " " : ""), $2, $1 }')
[[ -z "$TYPES" ]] || echo "TYPES: $TYPES"

# status <rev> <path>: 前付けの status の値（無ければ -）。前付けの形は design-records（ADR-0009）が持つ
status() {
  local text
  text=$(git show "$1:$2")
  design-records status <<<"$text"
}

# report <dir> <prefix>: dir 直下の NNNN-*.md の追加と status の変化。
# 改名（R）は新しいファイルとして数えず（claude-hooks のマージ前の判定と同じ）、改名前の status と比べる。
# 改名の検出は diff.renames の設定に左右されないよう --find-renames で明示する
report() {
  local dir="$1" prefix="$2" kind src path old new
  while IFS=$'\t' read -r kind src path; do
    [[ -n "$path" ]] || path="$src"
    [[ "$path" =~ ^$dir/[0-9]{4}-[^/]*\.md$ ]] || continue
    case "$kind" in
      A)
        # echo の引数の中のコマンド置換は、失敗しても set -e に掛からないので先に受ける
        new=$(status "$TIP" "$path")
        echo "${prefix}_NEW: $path ($new)"
        ;;
      M | R*)
        old=$(status "$FORK" "$src")
        new=$(status "$TIP" "$path")
        [[ "$old" == "$new" ]] || echo "${prefix}_STATUS: $path $old -> $new"
        ;;
    esac
  # 非 ASCII のパスをクォートさせない（NNNN-*.md の判定と出力のため）
  done < <(git -c core.quotePath=false diff --find-renames --diff-filter=AMR --name-status "$FORK" "$TIP" -- "$dir")
}
report docs/adr ADR
report docs/issues ISSUE

DIRS=$(git -c core.quotePath=false diff --name-only "$FORK" "$TIP" \
  | awk -F/ '{ if (NF == 1) print "."; else if (NF == 2) print $1; else print $1 "/" $2 }' \
  | sort -u | paste -sd ' ')
[[ -z "$DIRS" ]] || echo "DIRS: $DIRS"
