#!/usr/bin/env bash
# .claude/scripts の git 操作（worktree-add / rename-branch / rename-plan / git-info / post-merge-cleanup） のテスト
#
# 実行: bash .claude/tests/scripts/git-workflow.sh（まとめて実行: bash .claude/tests/scripts.sh）
#
# 実物の gh・ruff 等は使わず、make_fake_tool で PATH 先頭に置いた偽コマンドで
# 「スクリプトが何を呼び、出力をどう判定するか」を検証する。

source "$(dirname "$0")/../lib.sh"

# gh を呼ばれたら失敗する状態を既定にする（呼ばれないことの検証にもなる）
make_fake_gh ''

# ===========================================================================
# worktree-add.sh
# ===========================================================================

REPO="$TEST_ROOT/wt/app"
make_repo "$REPO"
make_remote "$REPO"
cd "$REPO" || exit 1

it "worktree-add: 新規ブランチを ../<repo>-<branch> に作る（/ は - に変換）"
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/login)
assert_eq 0 $?
assert_contains "$out" "WORKTREE: $TEST_ROOT/wt/app-feat-login"
assert_contains "$out" "CREATED_BRANCH: yes"
assert_eq "feat/login" "$(git -C "$TEST_ROOT/wt/app-feat-login" branch --show-current)"

it "worktree-add: 既存ブランチならそれをチェックアウトする"
git branch fix/typo
out=$("$SCRIPTS_DIR/worktree-add.sh" fix/typo)
assert_contains "$out" "CREATED_BRANCH: no"
assert_eq "fix/typo" "$(git -C "$TEST_ROOT/wt/app-fix-typo" branch --show-current)"

it "worktree-add: リモートにだけあるブランチは追跡ブランチとして作る"
git -C "$TEST_ROOT/wt/app-feat-login" push -q -u origin feat/login
git worktree remove "$TEST_ROOT/wt/app-feat-login"
git branch -D -q feat/login
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/login)
assert_eq "feat/login" "$(git -C "$TEST_ROOT/wt/app-feat-login" branch --show-current)"
assert_eq "origin/feat/login" "$(git -C "$TEST_ROOT/wt/app-feat-login" rev-parse --abbrev-ref '@{u}')"

it "worktree-add: 作成先が既にあればエラー"
"$SCRIPTS_DIR/worktree-add.sh" feat/login >/dev/null 2>&1
assert_eq 1 $?

it "worktree-add: --carry-changes で未コミットの変更（未追跡含む）を worktree 側へ移す"
echo modified >> README.md
echo new > new.txt
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/carry --carry-changes)
assert_eq 0 $?
assert_eq "" "$(git status --porcelain)"
assert_contains "$(git -C "$TEST_ROOT/wt/app-feat-carry" status --porcelain)" " M README.md"
assert_contains "$(git -C "$TEST_ROOT/wt/app-feat-carry" status --porcelain)" "?? new.txt"

it "worktree-add: リポジトリのサブディレクトリから実行しても親ディレクトリに作る"
mkdir -p sub && cd sub || exit 1
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/from-sub)
assert_contains "$out" "WORKTREE: $TEST_ROOT/wt/app-feat-from-sub"
cd "$REPO" || exit 1

it "worktree-add: 新しいブランチは今の HEAD ではなく既定ブランチ（origin/main）から作り、上流を設定しない"
# 作業中のブランチの未完了のコミットを別のトピックのブランチに持ち込まない
git switch -q -c feat/wip
commit_file "$REPO" "wip.txt" "feat: wip"
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/other)
assert_eq 0 $?
assert_contains "$out" "BASE: origin/main"
assert_eq "$(git rev-parse origin/main)" "$(git -C "$TEST_ROOT/wt/app-feat-other" rev-parse HEAD)"
# 上流が origin/main だと、引数なしの git push が main 宛てとして扱われる
assert_eq "" "$(git -C "$TEST_ROOT/wt/app-feat-other" rev-parse --abbrev-ref '@{u}' 2>/dev/null)"

it "worktree-add: --from で起点を明示できる（--from HEAD は今のブランチの続き）"
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/continue --from HEAD)
assert_eq 0 $?
assert_contains "$out" "BASE: HEAD"
assert_eq "$(git rev-parse feat/wip)" "$(git -C "$TEST_ROOT/wt/app-feat-continue" rev-parse HEAD)"

it "worktree-add: --from の ref が無ければエラーで、worktree を作らない"
"$SCRIPTS_DIR/worktree-add.sh" feat/nobase --from no-such-ref >/dev/null 2>&1
assert_eq 1 $?
assert_file_missing "$TEST_ROOT/wt/app-feat-nobase"

it "worktree-add: 既存のブランチに --from を付けたらエラー（起点は新しいブランチにだけ効く）"
git branch fix/exists main
"$SCRIPTS_DIR/worktree-add.sh" fix/exists --from HEAD >/dev/null 2>&1
assert_eq 1 $?
assert_file_missing "$TEST_ROOT/wt/app-fix-exists"

it "worktree-add: origin/HEAD があればそれを既定ブランチとする"
git push -q origin main:trunk
git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/trunk
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/on-trunk)
assert_contains "$out" "BASE: origin/trunk"
git symbolic-ref --delete refs/remotes/origin/HEAD
git switch -q main

LOCAL="$TEST_ROOT/wt/local"
make_repo "$LOCAL"
cd "$LOCAL" || exit 1

it "worktree-add: リモートが無ければローカルの main から作る"
git switch -q -c feat/wip
commit_file "$LOCAL" "wip.txt" "feat: wip"
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/other)
assert_contains "$out" "BASE: main"
assert_eq "$(git rev-parse main)" "$(git -C "$TEST_ROOT/wt/local-feat-other" rev-parse HEAD)"

NOMAIN="$TEST_ROOT/wt/nomain"
mkdir -p "$NOMAIN"
git -C "$NOMAIN" init -q -b work
commit_file "$NOMAIN" "a.txt" "chore: init"
cd "$NOMAIN" || exit 1

it "worktree-add: 既定ブランチが見つからなければエラーで、--from を案内する"
out=$("$SCRIPTS_DIR/worktree-add.sh" feat/x 2>&1)
assert_eq 1 $?
assert_contains "$out" "--from"
assert_file_missing "$TEST_ROOT/wt/nomain-feat-x"
cd "$REPO" || exit 1

# ===========================================================================
# rename-branch.sh
# ===========================================================================

REPO="$TEST_ROOT/rename/app"
make_repo "$REPO"
make_remote "$REPO" github
cd "$REPO" || exit 1

it "rename-branch: main では拒否する"
"$SCRIPTS_DIR/rename-branch.sh" feat/x >/dev/null 2>&1
assert_eq 1 $?

it "rename-branch: ローカルのみのブランチはそのままリネームする"
git switch -q -c feat/old
out=$("$SCRIPTS_DIR/rename-branch.sh" feat/new)
assert_eq 0 $?
assert_eq "feat/new" "$(git branch --show-current)"
assert_contains "$out" "REMOTE: none"

it "rename-branch: リモートにあるブランチは --remote なしでは止まる（exit 2）"
git push -q -u origin feat/new
"$SCRIPTS_DIR/rename-branch.sh" feat/newer >/dev/null 2>&1
assert_eq 2 $?
assert_eq "feat/new" "$(git branch --show-current)"

it "rename-branch: --remote でリモートも更新する（open PR がないとき）"
make_fake_gh '"pr view feat/new"*) echo "no pull requests found" >&2; exit 1 ;;'
out=$("$SCRIPTS_DIR/rename-branch.sh" feat/newer --remote)
assert_eq 0 $?
assert_eq "feat/newer" "$(git branch --show-current)"
assert_eq "" "$(git ls-remote --heads origin feat/new)"
assert_contains "$(git ls-remote --heads origin feat/newer)" "refs/heads/feat/newer"
assert_eq "origin/feat/newer" "$(git rev-parse --abbrev-ref '@{u}')"
assert_contains "$out" "REMOTE: updated"

it "rename-branch: open PR の head ブランチは --remote でもリネームしない（PR が閉じるため）"
make_fake_gh '"pr view feat/newer"*) echo OPEN ;;'
"$SCRIPTS_DIR/rename-branch.sh" feat/x --remote >/dev/null 2>&1
assert_eq 1 $?
assert_eq "feat/newer" "$(git branch --show-current)"

# ===========================================================================
# rename-plan.sh
# ===========================================================================

REPO="$TEST_ROOT/plan/app"
make_repo "$REPO"
cd "$REPO" || exit 1

it "rename-plan: docs/plans がなければ何もせず exit 0"
out=$("$SCRIPTS_DIR/rename-plan.sh" --list)
assert_eq 0 $?
assert_eq "" "$out"

mkdir -p docs/plans

# docs/plans はあるが .md が 1 つもない状態。*.md が展開されず ls が exit 2 を返し、
# pipefail + set -e でスクリプトごと落ちていた（git-branch スキルの読み込みが失敗した）
it "rename-plan: docs/plans が空でも --list は exit 0 で何も出さない"
out=$("$SCRIPTS_DIR/rename-plan.sh" --list)
assert_eq 0 $?
assert_eq "" "$out"

for n in 001_a 002_b 003_c; do echo "# $n" > "docs/plans/$n.md"; done
echo "# random plan" > docs/plans/optimized-cooking-mochi.md
git add docs && git commit -q -m "docs: plans"

it "rename-plan: --list は連番のない計画書だけを出す"
out=$("$SCRIPTS_DIR/rename-plan.sh" --list)
assert_eq "docs/plans/optimized-cooking-mochi.md" "$out"

it "rename-plan: ブランチ名から NNN_name.md を作り git mv してコミットする"
git switch -q -c refactor/skills-markdown-lint
out=$("$SCRIPTS_DIR/rename-plan.sh" refactor/skills-markdown-lint)
assert_eq 0 $?
assert_file_exists docs/plans/004_skills_markdown_lint.md
assert_file_missing docs/plans/optimized-cooking-mochi.md
assert_contains "$out" "COMMITTED: yes"
assert_contains "$(git log -1 --pretty=%s)" "docs: rename plan"
assert_eq "" "$(git status --porcelain)"

it "rename-plan: 対象がなければ何もせず exit 0"
out=$("$SCRIPTS_DIR/rename-plan.sh" refactor/skills-markdown-lint)
assert_eq 0 $?
assert_contains "$out" "NOTE"

it "rename-plan: 未追跡（gitignore 等）の計画書は mv だけ行いコミットしない"
echo "docs/plans/*.md" > .gitignore
echo "# ignored" > docs/plans/quiet-river.md
head=$(git rev-parse HEAD)
out=$("$SCRIPTS_DIR/rename-plan.sh" feat/ignored-plan)
assert_eq 0 $?
assert_file_exists docs/plans/005_ignored_plan.md
assert_contains "$out" "COMMITTED: no"
assert_eq "$head" "$(git rev-parse HEAD)"
rm .gitignore docs/plans/005_ignored_plan.md

it "rename-plan: ランダム名が複数あれば --file の指定を求めて exit 1"
echo a > docs/plans/alpha-bravo.md
echo b > docs/plans/charlie-delta.md
"$SCRIPTS_DIR/rename-plan.sh" feat/two >/dev/null 2>&1
assert_eq 1 $?
out=$("$SCRIPTS_DIR/rename-plan.sh" feat/two --file docs/plans/charlie-delta.md)
assert_eq 0 $?
assert_file_exists docs/plans/005_two.md
assert_file_exists docs/plans/alpha-bravo.md

# ===========================================================================
# git-info.sh
# ===========================================================================

REPO="$TEST_ROOT/info/app"
make_repo "$REPO"
make_remote "$REPO"
cd "$REPO" || exit 1
# 呼ばれないことを検証するため記録を空にしておく
make_fake_gh ''

it "git-info: 各セクションを見出し付きで出力する"
out=$("$SCRIPTS_DIR/git-info.sh")
assert_eq 0 $?
for h in "📍 現在のブランチ" "📝 未コミット変更" "📤 未プッシュコミット" "📦 stash" "🌳 worktree" "🔗 関連PR"; do
  assert_contains "$out" "$h"
done

it "git-info: 未プッシュコミットと未コミット変更を表示する"
git switch -q -c feat/x
git push -q -u origin feat/x
commit_file "$REPO" a.txt "feat: a"
echo dirty > dirty.txt
out=$("$SCRIPTS_DIR/git-info.sh")
assert_contains "$out" "feat: a"
assert_contains "$out" "?? dirty.txt"
rm dirty.txt

it "git-info: GitHub リモートがなければ gh を呼ばない"
assert_eq "" "$(fake_log gh)"

it "git-info: マージ済みブランチの worktree を検出して削除方法を示す"
git switch -q main
git branch -q feat/done
git worktree add -q "$TEST_ROOT/info/app-feat-done" feat/done
out=$("$SCRIPTS_DIR/git-info.sh")
assert_contains "$out" "マージ済みブランチのworktree"
assert_contains "$out" "app-feat-done (feat/done)"
git worktree remove "$TEST_ROOT/info/app-feat-done"
git branch -d -q feat/done

it "git-info: GitHub リモートがあれば gh pr status を表示する"
git remote add upstream https://github.com/example/app.git
make_fake_gh '"pr status"*) echo "PR_STATUS_OUTPUT" ;;'
out=$("$SCRIPTS_DIR/git-info.sh")
assert_contains "$out" "PR_STATUS_OUTPUT"

# ===========================================================================
# post-merge-cleanup.sh
# ===========================================================================

setup_merged_branch() {
  # <root>/app に main と、main へマージ済みで origin にも存在する feat/x を用意し、
  # feat/x を <root>/app-feat-x の worktree にチェックアウトした状態を作る
  local root="$1" kind="${2:-local}"
  local repo="$root/app"
  make_repo "$repo"
  make_remote "$repo" "$kind"
  git -C "$repo" switch -q -c feat/x
  commit_file "$repo" a.txt "feat: a"
  git -C "$repo" push -q -u origin feat/x
  git -C "$repo" switch -q main
  git -C "$repo" merge -q --no-ff -m "Merge: feat/x" feat/x
  git -C "$repo" push -q origin main
  git -C "$repo" worktree add -q "$root/app-feat-x" feat/x
}

it "post-merge-cleanup: worktree・ローカル・リモートのブランチを削除し main を最新化する"
setup_merged_branch "$TEST_ROOT/cleanup1"
cd "$TEST_ROOT/cleanup1/app" || exit 1
out=$("$SCRIPTS_DIR/post-merge-cleanup.sh" feat/x)
assert_eq 0 $?
assert_file_missing "$TEST_ROOT/cleanup1/app-feat-x"
assert_eq "" "$(git branch --list feat/x)"
assert_eq "" "$(git ls-remote --heads origin feat/x)"
assert_eq "main" "$(git branch --show-current)"
assert_contains "$out" "WORKTREE_REMOVED: $TEST_ROOT/cleanup1/app-feat-x"
assert_contains "$out" "LOCAL_BRANCH: deleted"
assert_contains "$out" "REMOTE_BRANCH: deleted"

it "post-merge-cleanup: worktree 側から実行しても動く（main 側へ移ってから削除）"
setup_merged_branch "$TEST_ROOT/cleanup2"
cd "$TEST_ROOT/cleanup2/app-feat-x" || exit 1
out=$("$SCRIPTS_DIR/post-merge-cleanup.sh" feat/x)
assert_eq 0 $?
assert_file_missing "$TEST_ROOT/cleanup2/app-feat-x"
assert_eq "" "$(git -C "$TEST_ROOT/cleanup2/app" branch --list feat/x)"
cd "$TEST_ROOT" || exit 1

it "post-merge-cleanup: リモートブランチを head とする open PR があれば削除しない"
setup_merged_branch "$TEST_ROOT/cleanup3" github
cd "$TEST_ROOT/cleanup3/app" || exit 1
make_fake_gh '"pr list --head feat/x --state open"*) echo 1 ;;'
out=$("$SCRIPTS_DIR/post-merge-cleanup.sh" feat/x)
assert_eq 0 $?
assert_contains "$(git ls-remote --heads origin feat/x)" "refs/heads/feat/x"
assert_contains "$out" "REMOTE_BRANCH: kept"

it "post-merge-cleanup: GitHub リモートで open PR がなければリモートも削除する"
setup_merged_branch "$TEST_ROOT/cleanup4" github
cd "$TEST_ROOT/cleanup4/app" || exit 1
make_fake_gh '"pr list --head feat/x --state open"*) echo 0 ;;'
out=$("$SCRIPTS_DIR/post-merge-cleanup.sh" feat/x)
assert_eq "" "$(git ls-remote --heads origin feat/x)"
assert_contains "$out" "REMOTE_BRANCH: deleted"

it "post-merge-cleanup: リモートのないリポジトリ（/git-merge の後）でも worktree とブランチを片付ける"
REPO="$TEST_ROOT/cleanup6/app"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
commit_file "$REPO" a.txt "feat: a"
git -C "$REPO" switch -q main
git -C "$REPO" merge -q --no-ff -m "Merge: feat/x" feat/x
git -C "$REPO" worktree add -q "$TEST_ROOT/cleanup6/app-feat-x" feat/x
cd "$REPO" || exit 1
out=$("$SCRIPTS_DIR/post-merge-cleanup.sh" feat/x)
assert_eq 0 $?
assert_file_missing "$TEST_ROOT/cleanup6/app-feat-x"
assert_eq "" "$(git branch --list feat/x)"
assert_contains "$out" "REMOTE_BRANCH: none"

it "post-merge-cleanup: 未マージのブランチは削除せず失敗する"
REPO="$TEST_ROOT/cleanup5/app"
make_repo "$REPO"
make_remote "$REPO"
git -C "$REPO" switch -q -c feat/unmerged
commit_file "$REPO" a.txt "feat: a"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
"$SCRIPTS_DIR/post-merge-cleanup.sh" feat/unmerged >/dev/null 2>&1
assert_eq 1 $?
assert_contains "$(git branch --list feat/unmerged)" "feat/unmerged"

finish
