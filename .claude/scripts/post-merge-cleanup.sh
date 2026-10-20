#!/usr/bin/env bash
# post-merge-cleanup.sh — PR マージ後（または /git-merge の後）にブランチと worktree を片付ける
#
# Usage: post-merge-cleanup.sh <branch>
#
# 順に行う（gh-pr-merge スキルの「クリーンアップ」）:
#   1. <branch> をチェックアウトしている worktree があれば削除（変更が残っていれば失敗する）
#      どの worktree から実行しても、既定ブランチの worktree に移ってから行う
#   2. 既定ブランチ（main / master）へ切り替え、fetch --prune、pull --ff-only（リモートがなければ省く）
#      （ローカルとリモートの履歴が分岐していれば ff できずに失敗し、気づける）
#      nixfiles で、pull で rebuild か claude-sync で配置するもの（hook、.claude の下）が変われば、NOTE で知らせる
#   3. origin から予約のタグ（adr-NNNN、issue-NNNN。ほかの clone が push したものを含む）を取ってきて、あれば、
#      既定ブランチの履歴に記録が入った番号のタグを、ローカルとリモートから消す（design-records prune。
#      ADR-0018）。design-records が無いか origin に届かなければ残して続け、prune が失敗したら ERROR で止まる。
#      既定ブランチが origin を追うときは、origin に届かなければ 2 の fetch で止まる（既定ブランチを最新にできず、
#      5 のリモートブランチの削除もできないので、続けない）。3 で残して続けるのは、既定ブランチがほかのリモート
#      （fork の upstream）を追うとき
#   4. ローカルブランチを削除（-d。未マージなら失敗する）
#   5. リモートブランチを削除。ただし、リモートの既定ブランチがブランチの先端を含まないとき（push 前）と、
#      GitHub リモートでそのブランチを head とする open PR（fork からの upstream PR 等）があるとき
#      （削除で PR が閉じる）は、削除せず残す
#
# 出力:
#   WORKTREE_REMOVED: <path>|none
#   NOTE: rebuild が要る (...) / NOTE: claude-sync が要る (...)（該当するときだけ。0 行以上）
#   RESERVATIONS_RELEASED: <tag>...|none|skipped (...)|kept (...)
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
# 以降は既定ブランチの worktree（どこにもチェックアウトされていなければメインの worktree）で行う。
# 今いる worktree が対象のものでも、無関係なブランチのものでも、そこでは既定ブランチへ switch できない
DEFAULT_ROOT=$(git worktree list --porcelain | awk -v ref="refs/heads/$DEFAULT" '
  /^worktree / { path = substr($0, 10) }
  /^branch /   { if ($2 == ref) print path }')
cd "${DEFAULT_ROOT:-$MAIN_ROOT}"
if [[ -n "$WT_PATH" ]]; then
  git worktree remove "$WT_PATH"
  echo "WORKTREE_REMOVED: $WT_PATH"
else
  echo "WORKTREE_REMOVED: none"
fi

# --- 2. 既定ブランチを最新化 ---
if [[ "$(git branch --show-current)" != "$DEFAULT" ]]; then
  git switch -q "$DEFAULT"
fi
# リモートのないリポジトリ（/git-merge でローカルにマージした後）では最新化する先がない
if [[ -n "$(git remote)" ]]; then
  git fetch -q --prune
  BEFORE=$(git rev-parse HEAD)
  git pull -q --ff-only
  # hook のバイナリと ~/.claude は rebuild か claude-sync で配置される。pull しただけでは古いまま動くので
  # 知らせる。自動では走らせない（rebuild は sudo が要り、走っている別のセッションの hook も入れ替わる）。
  # 配置の元は claude-sync.sh を持つリポジトリ（nixfiles）だけで、ほかのリポジトリの .claude はその場で読まれる
  if git cat-file -e HEAD:modules/home/scripts/claude-sync.sh 2>/dev/null; then
    # design-records は claude-hooks がパス依存でバイナリに組み込む（claude-hooks/package.nix）
    if [[ -n "$(git diff --name-only "$BEFORE" HEAD -- modules/home/parts/claude-hooks \
      modules/home/parts/design-records modules/home/parts/claude-code.nix)" ]]; then
      echo "NOTE: rebuild が要る（claude-hooks、design-records か claude-code.nix が変わった）。nixos-rebuild switch まで hook と設定は古い版のまま動く"
    fi
    # claude-sync が同期するものだけ（claude-sync.sh）
    if [[ -n "$(git diff --name-only "$BEFORE" HEAD -- .claude/CLAUDE.md .claude/commands .claude/skills .claude/scripts)" ]]; then
      echo "NOTE: claude-sync が要る（.claude の CLAUDE.md、commands、skills か scripts が変わった）。claude-sync か rebuild まで ~/.claude は古い版のまま"
    fi
  fi
fi

# --- 3. 番号の予約のタグ ---
# 記録の形と番号の規則は design-records が持つ。既定ブランチ（リモートがあればリモートの）の履歴に無い記録の
# タグは消さない（消すと番号が使い回される）。解放できないときは、タグを残して後片付けを続ける（タグが残っても
# 番号は使い回されず、次の prune で消える）。prune そのものが失敗したら止める（原因を直して走らせ直せる）
# 予約のタグは origin にある。2 の fetch は既定ブランチの追うリモートに行くので、fork のように upstream を
# 追っていると origin に行かない。ここで origin から取ってくる
ORIGIN_REACHABLE=1
if git remote get-url origin >/dev/null 2>&1 &&
  ! git fetch -q --no-tags origin 'refs/tags/adr-*:refs/tags/adr-*' 'refs/tags/issue-*:refs/tags/issue-*' 2>/dev/null; then
  ORIGIN_REACHABLE=0
fi
if [[ -z "$(git tag --list 'adr-*' 'issue-*')" ]]; then
  echo "RESERVATIONS_RELEASED: none"
elif ! command -v design-records >/dev/null 2>&1; then
  echo "RESERVATIONS_RELEASED: skipped (design-records が PATH に無い。予約のタグは残した)"
elif ((!ORIGIN_REACHABLE)); then
  echo "RESERVATIONS_RELEASED: kept (origin に届かない。予約のタグは残した)"
elif RELEASED=$(design-records prune); then
  RELEASED=${RELEASED:-none}
  echo "RESERVATIONS_RELEASED: ${RELEASED//$'\n'/ }"
else
  echo "ERROR: 予約のタグを解放できません（design-records prune が失敗）。原因を直してから、もう一度実行してください" >&2
  exit 1
fi

# --- 4. ローカルブランチ ---
if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
  git branch -d -q "$BRANCH"
  echo "LOCAL_BRANCH: deleted"
else
  echo "LOCAL_BRANCH: none"
fi

# --- 5. リモートブランチ ---
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
  # リモートの既定ブランチがブランチの先端を含まなければ、消すとマージした内容がリモートのどの ref にも残らない
  # （ローカルでマージしただけで push していない）。push するかはユーザーが決めるので、残して知らせる
  if ! git merge-base --is-ancestor "$REMOTE/$BRANCH" "$REMOTE/$DEFAULT" 2>/dev/null; then
    echo "REMOTE_BRANCH: kept ($REMOTE/$DEFAULT がブランチの先端をまだ含まない。$DEFAULT を push してから消す)"
    exit 0
  fi
  git push -q "$REMOTE" --delete "$BRANCH"
  echo "REMOTE_BRANCH: deleted"
else
  echo "REMOTE_BRANCH: none"
fi
