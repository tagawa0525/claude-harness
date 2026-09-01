#!/usr/bin/env bash
# .claude/scripts と skills/*/scripts のテスト
#
# 実行: bash .claude/tests/scripts.sh
# 対象: worktree-add.sh / rename-branch.sh / rename-plan.sh / git-info.sh /
#       post-merge-cleanup.sh / gh-actions-diagnose.sh /
#       language-checks/scripts/run-checks.sh / gh-pr-review/scripts/get-pr-info.sh
#
# 実物の gh・ruff 等は使わず、make_fake_tool で PATH 先頭に置いた偽コマンドで
# 「スクリプトが何を呼び、出力をどう判定するか」を検証する。

source "$(dirname "$0")/lib.sh"

LANG_SCRIPTS="$CLAUDE_DIR/skills/language-checks/scripts"
REVIEW_SCRIPTS="$CLAUDE_DIR/skills/gh-pr-review/scripts"

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

# ===========================================================================
# gh-actions-diagnose.sh
# ===========================================================================

REPO="$TEST_ROOT/actions/app"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1

RUN_FAIL='[{"databaseId":100,"name":"CI","status":"completed","conclusion":"failure","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/100"}]'
JOBS_FAIL='{"jobs":[{"name":"build","conclusion":"failure","steps":[{"name":"Setup","conclusion":"success"},{"name":"Test","conclusion":"failure"}]}]}'

it "gh-actions-diagnose: run がなければ CAUSE: NONE と未設定の旨を出す"
make_fake_gh '"run list --branch feat/x"*) echo "[]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_eq 0 $?
assert_contains "$out" "CAUSE: NONE"
assert_contains "$out" "未設定"

it "gh-actions-diagnose: 実行中なら CAUSE: IN_PROGRESS"
make_fake_gh '"run list --branch feat/x"*) echo "[{\"databaseId\":1,\"name\":\"CI\",\"status\":\"in_progress\",\"conclusion\":\"\",\"createdAt\":\"2026-09-01T00:00:00Z\",\"url\":\"u\"}]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: IN_PROGRESS"

it "gh-actions-diagnose: 成功なら CAUSE: NONE"
make_fake_gh '"run list --branch feat/x"*) echo "[{\"databaseId\":2,\"name\":\"CI\",\"status\":\"completed\",\"conclusion\":\"success\",\"createdAt\":\"2026-09-01T00:00:00Z\",\"url\":\"u\"}]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: NONE"

it "gh-actions-diagnose: 失敗ログに HTTP 5xx があれば TRANSIENT_API と再実行コマンドを出す"
make_fake_gh "\"run list --branch feat/x\"*) echo '$RUN_FAIL' ;;
  \"run view 100 --json jobs\"*) echo '$JOBS_FAIL' ;;
  \"run view 100 --log-failed\"*) printf 'build\tTest\t##[error]RequestError: HTTP 502 Bad Gateway\n' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_eq 0 $?
assert_contains "$out" "RUN: 100"
assert_contains "$out" "FAILED_JOB: build"
assert_contains "$out" "FAILED_STEP: Test"
assert_contains "$out" "CAUSE: TRANSIENT_API"
assert_contains "$out" "gh run rerun 100"

it "gh-actions-diagnose: Copilot の内部エラーなら COPILOT_INTERNAL"
make_fake_gh "\"run list --branch feat/x\"*) echo '$RUN_FAIL' ;;
  \"run view 100 --json jobs\"*) echo '$JOBS_FAIL' ;;
  \"run view 100 --log-failed\"*) printf 'review\tSetup\t##[error]Download ccrcli failed\n' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: COPILOT_INTERNAL"

it "gh-actions-diagnose: それ以外の失敗は CODE としてエラー行を出す"
make_fake_gh "\"run list --branch feat/x\"*) echo '$RUN_FAIL' ;;
  \"run view 100 --json jobs\"*) echo '$JOBS_FAIL' ;;
  \"run view 100 --log-failed\"*) printf 'build\tTest\t##[error]test_login failed: assertion error\n' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: CODE"
assert_contains "$out" "test_login failed"

it "gh-actions-diagnose: PR 番号を渡すと head ブランチを解決する"
make_fake_gh "\"pr view 7 --json headRefName\"*) echo feat/pr7 ;;
  \"run list --branch feat/pr7\"*) echo '[]' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh" 7)
assert_contains "$out" "BRANCH: feat/pr7"

it "gh-actions-diagnose: ブランチ名を渡すとそのブランチを見る"
make_fake_gh '"run list --branch other"*) echo "[]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh" other)
assert_contains "$out" "BRANCH: other"

# ===========================================================================
# language-checks/scripts/run-checks.sh
# ===========================================================================

REPO="$TEST_ROOT/checks/py"
make_repo "$REPO"
cd "$REPO" || exit 1
touch pyproject.toml

it "run-checks: Python プロジェクトで format → lint → test を順に実行し ALL_OK を出す"
make_fake_tool ruff '"format --check ."*) exit 0 ;; "check ."*) exit 0 ;;'
make_fake_tool pytest '*) exit 0 ;;'
mkdir -p tests && touch tests/test_a.py
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq $'format --check .\ncheck .' "$(fake_log ruff)"
assert_contains "$out" "ALL_OK"

it "run-checks: フォーマット失敗で止まり、修正コマンドを示す（lint は実行しない）"
make_fake_tool ruff '"format --check ."*) echo "would reformat a.py"; exit 1 ;; "check ."*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 1 $?
assert_eq "format --check ." "$(fake_log ruff)"
assert_contains "$out" "FAILED: python format"
assert_contains "$out" "FIX: ruff format ."

it "run-checks: テストがなければ pytest を飛ばす"
rm -r tests
make_fake_tool ruff '*) exit 0 ;;'
make_fake_tool pytest '*) echo "should not run"; exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "" "$(fake_log pytest)"

it "run-checks: ツールがなければ SKIP を出して続行する"
remove_fake_tool ruff
remove_fake_tool pytest
out=$(PATH="$(minimal_path)" "$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "SKIP: python format (ruff not found)"

it "run-checks: マーカーがなくてもステージ済みファイルの拡張子で言語を検出する"
REPO="$TEST_ROOT/checks/staged"
make_repo "$REPO"
cd "$REPO" || exit 1
echo '{ }' > x.nix && git add x.nix
make_fake_tool nixfmt '"--check"*) exit 0 ;;'
make_fake_tool statix '"check"*) exit 0 ;;'
make_fake_tool nix '*) echo "should not run without flake.nix"; exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$(fake_log nixfmt)" "--check"
assert_eq "check" "$(fake_log statix)"
assert_eq "" "$(fake_log nix)"

it "run-checks: 該当言語がなければ NOTE を出して exit 0"
REPO="$TEST_ROOT/checks/none"
make_repo "$REPO"
cd "$REPO" || exit 1
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "NOTE"

# ===========================================================================
# gh-pr-review/scripts/get-pr-info.sh: コメント URL の解析
# ===========================================================================

it "get-pr-info: コメント URL から PR 番号とコメント ID を取り出し PR 情報に添える"
make_fake_gh '"pr view 42 -R octo/repo --json"*) echo "{\"number\":42,\"title\":\"t\"}" ;;'
out=$("$REVIEW_SCRIPTS/get-pr-info.sh" "https://github.com/octo/repo/pull/42#discussion_r123456")
assert_eq 0 $?
assert_eq 42 "$(jq -r .number <<<"$out")"
assert_eq 123456 "$(jq -r .comment_id <<<"$out")"

it "get-pr-info: PR の URL（コメントなし）は comment_id が null"
out=$("$REVIEW_SCRIPTS/get-pr-info.sh" "https://github.com/octo/repo/pull/42")
assert_eq null "$(jq -r .comment_id <<<"$out")"

finish
