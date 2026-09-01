#!/usr/bin/env bash
# git-info.sh — 現在の Git 状態を俯瞰する（git-info スキルの収集・整形部分）
#
# Usage: git-info.sh
#
# ブランチ・未コミット変更・未プッシュコミット・stash・worktree・関連 PR を表示し、
# 既定ブランチ（main / master）にマージ済みのブランチを持つ worktree を検出する。
# 情報収集なので個別コマンドの失敗では止めず、代替表示にする。

set -uo pipefail

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "ERROR: git リポジトリではありません" >&2
  exit 1
fi

indent_or_none() {
  local text
  text=$(cat)
  if [[ -n "$text" ]]; then
    sed 's/^/   /' <<<"$text"
  else
    echo "   (なし)"
  fi
}

BRANCH=$(git branch --show-current 2>/dev/null)
[[ -n "$BRANCH" ]] || BRANCH="(detached HEAD: $(git rev-parse --short HEAD 2>/dev/null))"
UPSTREAM=$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || echo "")

echo "📍 現在のブランチ: $BRANCH"
if [[ -n "$UPSTREAM" ]]; then
  read -r AHEAD BEHIND < <(git rev-list --left-right --count "HEAD...$UPSTREAM" 2>/dev/null || echo "? ?")
  echo "   上流: $UPSTREAM (ahead $AHEAD / behind $BEHIND)"
else
  echo "   上流: (未設定)"
fi

echo
echo "📝 未コミット変更:"
git status --short 2>/dev/null | indent_or_none

echo
echo "📤 未プッシュコミット:"
if [[ -n "$UPSTREAM" ]]; then
  git log "$UPSTREAM..HEAD" --oneline 2>/dev/null | indent_or_none
else
  echo "   (上流ブランチ未設定)"
fi

echo
echo "📦 stash:"
git stash list 2>/dev/null | indent_or_none

echo
echo "🌳 worktree:"
git worktree list 2>/dev/null | indent_or_none

echo
echo "🔗 関連PR:"
if git remote -v 2>/dev/null | grep -q 'github\.com'; then
  if command -v gh >/dev/null 2>&1; then
    gh pr status 2>&1 | indent_or_none
  else
    echo "   (gh 未インストール)"
  fi
else
  echo "   (GitHub リモートなし)"
fi

# 既定ブランチにマージ済みのブランチを持つ linked worktree
DEFAULT=""
for b in main master; do
  if git show-ref --verify --quiet "refs/heads/$b"; then DEFAULT="$b"; break; fi
done
if [[ -n "$DEFAULT" ]]; then
  MAIN_ROOT=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
  MERGED=$(git branch --merged "$DEFAULT" --format='%(refname:short)' 2>/dev/null)
  STALE=()
  while IFS=$'\t' read -r path branch; do
    [[ "$path" == "$MAIN_ROOT" || -z "$branch" || "$branch" == "$DEFAULT" ]] && continue
    if grep -qxF -- "$branch" <<<"$MERGED"; then
      STALE+=("$path"$'\t'"$branch")
    fi
  done < <(git worktree list --porcelain | awk '
    /^worktree / { path = substr($0, 10) }
    /^branch /   { sub("refs/heads/", "", $2); printf "%s\t%s\n", path, $2 }
    /^detached/  { printf "%s\t\n", path }')

  if (( ${#STALE[@]} > 0 )); then
    echo
    echo "⚠️ マージ済みブランチのworktreeがあります:"
    echo
    for entry in "${STALE[@]}"; do
      IFS=$'\t' read -r path branch <<<"$entry"
      echo "- $path ($branch) - マージ済み"
    done
    echo
    echo "削除するには:"
    for entry in "${STALE[@]}"; do
      IFS=$'\t' read -r path branch <<<"$entry"
      echo "  /git-worktree $branch --remove"
    done
  fi
fi
