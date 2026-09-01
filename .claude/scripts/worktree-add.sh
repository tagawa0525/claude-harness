#!/usr/bin/env bash
# worktree-add.sh — 命名規則に従って worktree を作る
#
# Usage: worktree-add.sh <branch> [--carry-changes]
#
# 作成先はメイン worktree の親ディレクトリの `<repo>-<branch>`（ブランチ名の / は - に変換）。
# ブランチの状態は自動判定する:
#   - ローカルにある       → そのままチェックアウト
#   - リモートにだけある   → 追跡ブランチを作ってチェックアウト
#   - どこにもない         → HEAD から新規作成
# --carry-changes: 未コミットの変更（未追跡ファイル含む）を stash 経由で worktree 側に移す
#   （git-commit スキルの「main からの退避」）。
#
# 出力:
#   WORKTREE: <path>
#   BRANCH: <branch>
#   CREATED_BRANCH: yes|no
#   NOTE: ...（GitHub リモートが複数あるとき）

set -euo pipefail

usage() {
  echo "Usage: $0 <branch> [--carry-changes]" >&2
  exit 1
}

BRANCH=""
CARRY=0
while (( $# > 0 )); do
  case "$1" in
    --carry-changes) CARRY=1 ;;
    -*) usage ;;
    *)
      [[ -z "$BRANCH" ]] || usage
      BRANCH="$1"
      ;;
  esac
  shift
done
[[ -n "$BRANCH" ]] || usage

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "ERROR: git リポジトリではありません" >&2
  exit 1
fi

# メイン worktree（一覧の先頭）を基準にする。linked worktree やサブディレクトリから
# 実行しても同じ場所に作られる
MAIN_ROOT=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
REPO_NAME=$(basename "$MAIN_ROOT")
DIR="$(dirname "$MAIN_ROOT")/${REPO_NAME}-${BRANCH//\//-}"

if [[ -e "$DIR" ]]; then
  echo "ERROR: 作成先が既に存在します: $DIR" >&2
  exit 1
fi

CREATED=no
if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
  ADD_ARGS=("$DIR" "$BRANCH")
else
  REMOTE_REF=$(git for-each-ref --format='%(refname:short)' "refs/remotes/*/$BRANCH" | head -n 1)
  if [[ -n "$REMOTE_REF" ]]; then
    ADD_ARGS=("$DIR" --track -b "$BRANCH" "$REMOTE_REF")
  else
    ADD_ARGS=("$DIR" -b "$BRANCH")
    CREATED=yes
  fi
fi

# stash は worktree 間で共有される（common dir）ので、ここで退避して worktree 側で戻せる
STASHED=0
if (( CARRY )) && [[ -n "$(git status --porcelain)" ]]; then
  git stash push -q -u -m "wip: move to worktree $BRANCH"
  STASHED=1
fi

git worktree add -q "${ADD_ARGS[@]}"

if (( STASHED )); then
  if ! git -C "$DIR" stash pop -q --index; then
    echo "ERROR: 変更の適用に失敗しました。変更は stash に残っています（git stash list）" >&2
    exit 1
  fi
fi

echo "WORKTREE: $DIR"
echo "BRANCH: $BRANCH"
echo "CREATED_BRANCH: $CREATED"

# fork 運用（GitHub リモートが複数）では gh の解決先が worktree に引き継がれないことがある
GH_REMOTES=$(git remote -v | awk '/github\.com/ && /\(fetch\)/ { print $1 }' | sort -u | wc -l)
if (( GH_REMOTES >= 2 )); then
  echo "NOTE: GitHub リモートが複数あります。worktree で gh pr 系コマンドを使う前に gh repo set-default --view で解決先を確認してください"
fi
