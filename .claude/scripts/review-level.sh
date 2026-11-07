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
#   TARGET: <対象のブランチ>   差分を取るブランチ
#   WORKTREE: <パス | none>    TARGET をチェックアウトしている worktree。/code-review の対象に渡す。
#                              ブランチ名で渡すと /code-review は cwd のリポジトリでブランチを解くので、
#                              cwd が別のリポジトリ（や main の worktree）だと対象を見つけられず、
#                              「指摘なし」と同じ形の空の結果が返る。none はどこにもチェックアウトされて
#                              いない（呼び出し側がチェックアウトするか、PR の URL を渡す）
#   FILES: <変更ファイル数>
#   ROUNDS: 1 | 3              ローカルの /code-review の周の上限（記録の周と例外の周を除く）。差分のどのファイルも
#                              下の「文書」なら 1、ほかは 3。claude-hooks の cap-review-rounds が読み、上限に記録の周 1
#                              （3 のときはさらに例外の周 3）を足した数で止める
#   LEVEL: medium | high       ROUNDS が 1 なら medium、3 なら high。指摘の上限が高いと文書の周ごとに 8〜10 件が出て、
#                              指摘の数が収束の目安にならない
#
# 「文書」は、凍結せずマージの後も直せるので周を重ねて取り切るより後で直す方が安いもの:
#   - docs/ の下の、ADR（docs/adr/NNNN-*.md）でないファイル（issue、計画、説明の文書）
#   - 規則の文書: .claude/ の下の .md（スキルの文書、hook の一覧）と、どこにある CLAUDE.md も
#   - 既存の ADR の「## 変更」の節より後だけを変えるもの（決定と理由などの凍結する節や、フロントマターを
#     変えるものは入らない。新しい ADR、消す ADR、「## 変更」の節を足す ADR も入らない）
# docs/ の外の .md（検証の記録の数値など）とスキルの中のスクリプトは文書に数えない。
#
# 差分が空、または refs を解決できないときは exit 1。
#
# 移動（rename）は旧パスも数える（--no-renames）。src/ から docs/ への移動は、
# コードを消す変更なので文書だけの変更として扱わない。
# docs/ の判定は pre-merge-check（is_docs_only）と decide-next.sh（docs_only）の docs/ の前方一致と違い、
# ADR を除く（それらは Copilot の PR の自動レビューの扱いで、ローカルの周の上限とは別の判定）。

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

# ADR の「## 変更」の見出しの行番号（無ければ空）
change_heading_line() {
  git show "$1:$2" 2>/dev/null | grep -n -m1 '^## 変更[[:space:]]*$' | cut -d: -f1
}

# 既存の ADR の変更が「## 変更」の見出しより後だけか。-U0 の hunk の見出し @@ -a[,b] +c[,d] @@ で見る。
# 行数 0 の側（追加だけ、削除だけ）は「その行の後」を表すので見出しの行そのものも許す
adr_change_section_only() {
  local path=$1 base_line target_line
  git cat-file -e "${MERGE_BASE}:${path}" 2>/dev/null || return 1
  git cat-file -e "${TARGET}:${path}" 2>/dev/null || return 1
  base_line=$(change_heading_line "$MERGE_BASE" "$path")
  target_line=$(change_heading_line "$TARGET" "$path")
  [[ -n "$base_line" && -n "$target_line" ]] || return 1
  git diff -U0 --no-renames "$MERGE_BASE" "$TARGET" -- "$path" | awk -v ob="$base_line" -v nb="$target_line" '
    function after(start, count, heading) { return count == 0 ? start >= heading : start > heading }
    /^@@ / {
      split(substr($2, 2), o, ","); split(substr($3, 2), n, ",")
      if (!after(o[1] + 0, (2 in o) ? o[2] + 0 : 1, ob) || !after(n[1] + 0, (2 in n) ? n[2] + 0 : 1, nb)) bad = 1
    }
    END { exit bad }'
}

is_document() {
  local path=$1
  case "$path" in
    docs/adr/[0-9][0-9][0-9][0-9]-*.md) adr_change_section_only "$path" ;;
    docs/*) return 0 ;;
    .claude/*.md | CLAUDE.md | */CLAUDE.md) return 0 ;;
    *) return 1 ;;
  esac
}

MERGE_BASE=$(git merge-base "$BASE" "$TARGET")
rounds=1
while IFS= read -r path; do
  if ! is_document "$path"; then
    rounds=3
    break
  fi
done <<<"$files"

if ((rounds == 1)); then
  level=medium
else
  level=high
fi

# TARGET をチェックアウトしている worktree のパス
worktree=$(git worktree list --porcelain | awk -v ref="refs/heads/${TARGET}" '
  # 1 件の記録は空行で終わる。ディレクトリが消えた worktree（prunable）は対象にしない
  function flush() { if (hit && !gone && found == "") found = path; hit = 0; gone = 0 }
  $1 == "worktree" { path = substr($0, 10) }
  $1 == "branch" && $2 == ref { hit = 1 }
  $1 == "prunable" { gone = 1 }
  $0 == "" { flush() }
  END { flush(); if (found != "") print found }')

echo "BASE: ${BASE}"
echo "TARGET: ${TARGET}"
echo "WORKTREE: ${worktree:-none}"
echo "FILES: $(grep -c . <<<"$files")"
echo "ROUNDS: ${rounds}"
echo "LEVEL: ${level}"
