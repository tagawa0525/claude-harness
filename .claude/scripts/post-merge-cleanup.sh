#!/usr/bin/env bash
# post-merge-cleanup.sh — PR マージ後にブランチと worktree を片付ける
#
# Usage: post-merge-cleanup.sh <branch>
#
# 順に行う（gh-pr-merge スキルの「クリーンアップ」）:
#   1. <branch> をチェックアウトしている worktree があれば削除（変更が残っていれば失敗する）
#      自分がその worktree にいる場合はメイン worktree に移ってから行う
#   2. 既定ブランチ（main / master）へ切り替え、fetch --prune、pull --ff-only
#      （ローカルとリモートの履歴が分岐していれば ff できずに失敗し、気づける）
#   3. ローカルブランチを削除（-d。未マージなら失敗する）
#   4. リモートブランチを削除。ただし GitHub リモートでは、そのブランチを head とする
#      open PR（fork からの upstream PR 等）があると削除で PR が閉じるため、削除せず残す
#
# 出力:
#   WORKTREE_REMOVED: <path>|none
#   LOCAL_BRANCH: deleted|none
#   REMOTE_BRANCH: deleted|kept (...)|none

set -euo pipefail

BRANCH="${1:-}"
if [[ -z "$BRANCH" || $# -ne 1 ]]; then
  echo "Usage: $0 <branch>" >&2
  exit 1
fi

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "ERROR: git リポジトリではありません" >&2
  exit 1
fi

DEFAULT=""
for b in main master; do
  if git show-ref --verify --quiet "refs/heads/$b"; then DEFAULT="$b"; break; fi
done
if [[ -z "$DEFAULT" ]]; then
  echo "ERROR: 既定ブランチ（main / master）が見つかりません" >&2
  exit 1
fi
if [[ "$BRANCH" == "$DEFAULT" ]]; then
  echo "ERROR: 既定ブランチ $DEFAULT は削除しません" >&2
  exit 1
fi

# リモート名はブランチ削除前に読む（branch -d で設定も消える）
REMOTE=$(git config "branch.$BRANCH.remote" 2>/dev/null || echo origin)

# --- 1. worktree ---
MAIN_ROOT=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
WT_PATH=$(git worktree list --porcelain | awk -v ref="refs/heads/$BRANCH" '
  /^worktree / { path = substr($0, 10) }
  /^branch /   { if ($2 == ref) print path }')
if [[ -n "$WT_PATH" ]]; then
  if [[ "$(git rev-parse --show-toplevel)" == "$WT_PATH" ]]; then
    cd "$MAIN_ROOT"
  fi
  git worktree remove "$WT_PATH"
  echo "WORKTREE_REMOVED: $WT_PATH"
else
  echo "WORKTREE_REMOVED: none"
fi

# --- 2. 既定ブランチを最新化 ---
if [[ "$(git branch --show-current)" != "$DEFAULT" ]]; then
  git switch -q "$DEFAULT"
fi
git fetch -q --prune
git pull -q --ff-only

# --- 3. ローカルブランチ ---
if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
  git branch -d -q "$BRANCH"
  echo "LOCAL_BRANCH: deleted"
else
  echo "LOCAL_BRANCH: none"
fi

# --- 4. リモートブランチ ---
if git remote get-url "$REMOTE" >/dev/null 2>&1 \
  && [[ -n "$(git ls-remote --heads "$REMOTE" "$BRANCH" 2>/dev/null)" ]]; then
  if git remote -v | grep -q 'github\.com'; then
    if ! command -v gh >/dev/null 2>&1; then
      echo "REMOTE_BRANCH: kept (gh がないため open PR の有無を確認できない)"
      exit 0
    fi
    if ! OPEN_PRS=$(gh pr list --head "$BRANCH" --state open --json number --jq length 2>/dev/null); then
      echo "REMOTE_BRANCH: kept (open PR の有無を確認できない: gh pr list が失敗)"
      exit 0
    fi
    if (( OPEN_PRS > 0 )); then
      echo "REMOTE_BRANCH: kept (open PR ${OPEN_PRS} 件の head。削除すると PR が閉じる)"
      exit 0
    fi
  fi
  git push -q "$REMOTE" --delete "$BRANCH"
  echo "REMOTE_BRANCH: deleted"
else
  echo "REMOTE_BRANCH: none"
fi
