#!/usr/bin/env bash
# post-merge-cleanup.sh — PR マージ後（または /git-merge の後）にブランチと worktree を片付ける
#
# Usage: post-merge-cleanup.sh <branch>
#
# 順に行う（gh-pr-merge スキルの「クリーンアップ」）:
#   0. （1 の前）ローカルの <branch> の先端が既定ブランチ（なければ、追うリモートの既定ブランチを fetch した
#      もの）の祖先でなければ、何も消さずに ERROR で止まる（マージが途中で止まったのに続けて呼ぶと、1 で worktree
#      が、無視されたファイルごと消える。4 の -d で未マージに気づいても遅い）。ローカルのブランチが無ければ見ない
#   1. <branch> をチェックアウトしている worktree があれば削除（変更が残っていれば失敗する）
#      どの worktree から実行しても、既定ブランチの worktree に移ってから行う
#   2. 既定ブランチ（main / master）へ切り替え、fetch --prune、pull --ff-only（リモートがなければ省く）
#      （ローカルとリモートの履歴が分岐していれば ff できずに失敗し、気づける）
#      nixfiles で、pull で modules、hosts、flake か .claude が変われば、rebuild か claude-sync が要ると NOTE で知らせる
#   3. origin から予約のタグ（adr-NNNN、issue-NNNN。ほかの clone が push したものを含む）を取ってきて、あれば、
#      既定ブランチの履歴に記録が入った番号のタグを、ローカルとリモートから消す（design-records prune）。
#      design-records が無いか origin に届かなければ残して続け、prune が失敗したら ERROR で止まる。
#      既定ブランチが origin を追うときは、origin に届かなければ 2 の fetch で止まる（既定ブランチを最新にできず、
#      5 のリモートブランチの削除もできないので、続けない）。3 で残して続けるのは、既定ブランチがほかのリモート
#      （fork の upstream）を追うとき
#   4. ローカルブランチを削除（-d。0 で確かめたあとの安全網）
#   5. リモートブランチを削除。ただし、リモートの既定ブランチがブランチの先端を含まないとき（push 前）と、
#      GitHub リモートでそのブランチを head とする open PR（fork からの upstream PR 等）があるとき
#      （削除で PR が閉じる）は、削除せず残す
#
# 出力:
#   WORKTREE_REMOVED: <path>|none
#   NOTE: rebuild が要る（...）。... / NOTE: claude-sync が要る（...）。...（該当するときだけ。0 行以上）
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

# --- 0. マージ済みか ---
# 判定は先端が既定ブランチの祖先かどうか（squash や rebase のマージは、先端が履歴に残らないので未マージ扱い）。
# gh pr merge の後はローカルの既定ブランチがまだ pull されていないので、追うリモートの既定ブランチも fetch して
# 見る。fetch できなければ、その理由を添えて止める（マージ済みでもネットワークの失敗で止まりうる）。
# refs/heads/ を付けるのは、同名のタグが先に解決されて別のコミットで判定されるのを避けるため
if git show-ref --verify --quiet "refs/heads/$BRANCH" &&
  ! git merge-base --is-ancestor "refs/heads/$BRANCH" "refs/heads/$DEFAULT"; then
  MERGED=0
  FETCH_ERROR=""
  UPSTREAM=$(git for-each-ref --format='%(upstream:short)' "refs/heads/$DEFAULT")
  if [[ -n "$UPSTREAM" ]]; then
    UP_REMOTE=$(git config "branch.$DEFAULT.remote")
    if ! FETCH_ERROR=$(git fetch -q "$UP_REMOTE" 2>&1); then
      FETCH_ERROR="$UP_REMOTE の fetch に失敗した（${FETCH_ERROR//$'\n'/ }）"
    elif git merge-base --is-ancestor "refs/heads/$BRANCH" "refs/remotes/$UPSTREAM"; then
      MERGED=1
    fi
  fi
  if ((!MERGED)); then
    DETAIL="$DEFAULT${UPSTREAM:+、$UPSTREAM}の履歴にブランチの先端が無い"
    if [[ -n "$FETCH_ERROR" ]]; then
      DETAIL="$DETAIL。$FETCH_ERROR。マージ済みでも、$UPSTREAM を取れないと確かめられない"
    fi
    if [[ -n "$DEFAULT_ROOT" ]] && git rev-parse -q --verify MERGE_HEAD >/dev/null; then
      DETAIL="$DETAIL。$DEFAULT の worktree（$DEFAULT_ROOT）に MERGE_HEAD が残っている（マージが途中で止まっている）"
    fi
    {
      echo "ERROR: $BRANCH は $DEFAULT にまだマージされていません（$DETAIL）。何も消していません"
      echo "  修正方法: マージを終える（コンフリクトの解消や hook の拒否の原因を直して、もう一度マージする）。"
      echo "  やり直すなら、途中のマージを git merge --abort で取り消してから直す。そのあと、もう一度実行する"
    } >&2
    exit 1
  fi
fi

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
  # 知らせる。自動では実行しない（rebuild は sudo が要り、走っている別のセッションの hook も入れ替わる）。
  # 配置の元は claude-sync.sh を持つリポジトリ（nixfiles）だけで、ほかのリポジトリの .claude はその場で読まれる
  if git cat-file -e HEAD:modules/home/scripts/claude-sync.sh 2>/dev/null; then
    # 判定は広く取る。rebuild が配置するものを 1 つずつ挙げると漏れ、多めに知らせても害は無い
    if [[ -n "$(git diff --name-only "$BEFORE" HEAD -- modules hosts flake.nix flake.lock)" ]]; then
      echo "NOTE: rebuild が要る（modules、hosts か flake が変わった）。nix-rebuild rebuild まで hook や設定は古い版のまま動く"
    fi
    if [[ -n "$(git diff --name-only "$BEFORE" HEAD -- .claude)" ]]; then
      echo "NOTE: claude-sync が要る（.claude が変わった）。claude-sync か rebuild まで ~/.claude は古い版のまま"
    fi
  fi
fi

# --- 3. 番号の予約のタグ ---
# 記録の形と番号の規則は design-records が持つ。既定ブランチ（リモートがあればリモートの）の履歴に無い記録の
# タグは消さない（消すと番号が再利用される）。解放できないときは、タグを残して後片付けを続ける（タグが残っても
# 番号は再利用されず、次の prune で消える）。prune そのものが失敗したら止める（原因を直して再実行できる）
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
