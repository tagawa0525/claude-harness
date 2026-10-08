#!/usr/bin/env bash
# worktree-add.sh — 命名規則に従って worktree を作る
#
# Usage: worktree-add.sh <branch> [--from <ref>] [--carry-changes]
#
# 作成先はメイン worktree の親ディレクトリの `<repo>-<branch>`（ブランチ名の / は - に変換）。
# ブランチの状態は自動判定する:
#   - ローカルにある       → そのままチェックアウト
#   - リモートにだけある   → 追跡ブランチを作ってチェックアウト
#   - どこにもない         → 既定ブランチ（origin/HEAD → origin/main → origin/master → main → master
#                            の最初にあるもの）から上流なしで新規作成。今の HEAD から作ると、
#                            作業中のブランチの未完了のコミットが別のトピックに入るため
# --from <ref>: 新規作成の起点を明示する（今のブランチの続きなら --from HEAD）。
#   既存のブランチに付けたらエラー。
# --carry-changes: 未コミットの変更（未追跡ファイル含む）を stash 経由で worktree 側に移す
#   （git-commit スキルの「main からの退避」）。
#
# 出力:
#   WORKTREE: <path>
#   BRANCH: <branch>
#   CREATED_BRANCH: yes|no
#   BASE: <ref>（新規作成したときだけ。起点）
#   NOTE: ...（GitHub リモートが複数あるとき）

set -euo pipefail

usage() {
  echo "Usage: $0 <branch> [--from <ref>] [--carry-changes]" >&2
  exit 1
}

BRANCH=""
CARRY=0
FROM=""
while (( $# > 0 )); do
  case "$1" in
    --carry-changes) CARRY=1 ;;
    --from)
      (( $# >= 2 )) || usage
      FROM="$2"
      shift
      ;;
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

# 既定ブランチ。branch-topics.sh と claude-hooks（git.rs の default_branch）も同じ順で探す
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

CREATED=no
BASE=""
REMOTE_REF=""
if ! git show-ref --verify --quiet "refs/heads/$BRANCH"; then
  REMOTE_REF=$(git for-each-ref --format='%(refname:short)' "refs/remotes/*/$BRANCH" | head -n 1)
fi
if git show-ref --verify --quiet "refs/heads/$BRANCH" || [[ -n "$REMOTE_REF" ]]; then
  if [[ -n "$FROM" ]]; then
    echo "ERROR: ブランチ $BRANCH は既にあります。--from は新しいブランチにだけ指定できます" >&2
    exit 1
  fi
  if [[ -n "$REMOTE_REF" ]]; then
    ADD_ARGS=("$DIR" --track -b "$BRANCH" "$REMOTE_REF")
  else
    ADD_ARGS=("$DIR" "$BRANCH")
  fi
else
  if [[ -n "$FROM" ]]; then
    BASE="$FROM"
  elif ! BASE=$(default_branch); then
    echo "ERROR: 既定ブランチ（origin/HEAD, origin/main, origin/master, main, master）が見つかりません。--from <ref> で起点を指定してください" >&2
    exit 1
  fi
  if ! git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null; then
    echo "ERROR: 起点のコミットが見つかりません: $BASE" >&2
    exit 1
  fi
  # 起点がリモート追跡ブランチでも上流にしない（上流が origin/main だと push 先が main になる）
  ADD_ARGS=("$DIR" --no-track -b "$BRANCH" "$BASE")
  CREATED=yes
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
[[ -z "$BASE" ]] || echo "BASE: $BASE"

# fork 運用（GitHub リモートが複数）では gh の解決先が worktree に引き継がれないことがある
GH_REMOTES=$(git remote -v | awk '/github\.com/ && /\(fetch\)/ { print $1 }' | sort -u | wc -l)
if (( GH_REMOTES >= 2 )); then
  echo "NOTE: GitHub リモートが複数あります。worktree で gh pr 系コマンドを使う前に gh repo set-default --view で解決先を確認してください"
fi
