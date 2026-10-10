#!/usr/bin/env bash
# .claude/scripts/git-merge-state.sh（/git-merge の事前確認）のテスト
#
# 実行: bash .claude/tests/scripts/git-merge-state.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

SCRIPT="$SCRIPTS_DIR/git-merge-state.sh"

# local_repo <dir>: リモートのない main に feat/x（1 コミット）を足したリポジトリ。main に戻した状態
local_repo() {
  make_repo "$1"
  git -C "$1" switch -q -c feat/x
  commit_file "$1" a.txt "feat: a"
  git -C "$1" switch -q main
}

it "git-merge-state: main に追いついているブランチは BEHIND 0 で、コミット数を出す"
local_repo "$TEST_ROOT/s1"
cd "$TEST_ROOT/s1" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_contains "$out" "TARGET: feat/x"
assert_contains "$out" "DEFAULT: main"
assert_contains "$out" "COMMITS: 1"
assert_contains "$out" "BEHIND: 0"
assert_contains "$out" "STACKED_ON: none"
assert_contains "$out" "TARGET_WORKTREE: none"

it "git-merge-state: 引数を省くと今のブランチが対象"
git switch -q feat/x
out=$("$SCRIPT")
assert_contains "$out" "TARGET: feat/x"

it "git-merge-state: main が進んでいれば BEHIND に遅れのコミット数を出す"
git switch -q main
commit_file "$TEST_ROOT/s1" b.txt "feat: b"
out=$("$SCRIPT" feat/x)
assert_contains "$out" "BEHIND: 1"

it "git-merge-state: 別の未マージのブランチの上に積まれていれば STACKED_ON に出す"
local_repo "$TEST_ROOT/s2"
cd "$TEST_ROOT/s2" || exit 1
git switch -q feat/x
git switch -q -c feat/y
commit_file "$TEST_ROOT/s2" b.txt "feat: b"
git switch -q main
out=$("$SCRIPT" feat/y)
assert_contains "$out" "COMMITS: 2"
assert_contains "$out" "STACKED_ON: feat/x"

it "git-merge-state: ブランチの worktree があればそのパスを出す"
local_repo "$TEST_ROOT/s3"
cd "$TEST_ROOT/s3" || exit 1
git worktree add -q "$TEST_ROOT/s3-feat-x" feat/x
out=$("$SCRIPT" feat/x)
assert_contains "$out" "TARGET_WORKTREE: $TEST_ROOT/s3-feat-x"
assert_contains "$out" "DEFAULT_WORKTREE: $TEST_ROOT/s3"

it "git-merge-state: 対象の worktree の中から実行しても、既定ブランチの worktree を出す"
out=$(cd "$TEST_ROOT/s3-feat-x" && "$SCRIPT")
assert_contains "$out" "DEFAULT_WORKTREE: $TEST_ROOT/s3"

it "git-merge-state: 対象の worktree に未コミットの変更があればエラー"
echo dirty >> "$TEST_ROOT/s3-feat-x/a.txt"
out=$("$SCRIPT" feat/x 2>&1)
assert_eq 1 $?
assert_contains "$out" "未コミット"

it "git-merge-state: 既定ブランチの worktree に未コミットの変更があればエラー"
git -C "$TEST_ROOT/s3-feat-x" checkout -q -- a.txt
echo dirty >> "$TEST_ROOT/s3/README.md"
out=$("$SCRIPT" feat/x 2>&1)
assert_eq 1 $?
assert_contains "$out" "$TEST_ROOT/s3"
git -C "$TEST_ROOT/s3" checkout -q -- README.md

it "git-merge-state: main / master 自身は対象にしない"
local_repo "$TEST_ROOT/s4"
cd "$TEST_ROOT/s4" || exit 1
out=$("$SCRIPT" main 2>&1)
assert_eq 1 $?
assert_contains "$out" "既定ブランチ"

it "git-merge-state: 存在しないブランチはエラー"
out=$("$SCRIPT" feat/none 2>&1)
assert_eq 1 $?
assert_contains "$out" "feat/none"

it "git-merge-state: GitHub リモートがあれば /gh-pr-create と /gh-pr-merge を案内して失敗する"
local_repo "$TEST_ROOT/s5"
make_remote "$TEST_ROOT/s5" github
cd "$TEST_ROOT/s5" || exit 1
out=$("$SCRIPT" feat/x 2>&1)
assert_eq 1 $?
assert_contains "$out" "/gh-pr-create"

it "git-merge-state: GitHub 以外のリモートだけなら続行する"
local_repo "$TEST_ROOT/s6"
make_remote "$TEST_ROOT/s6"
cd "$TEST_ROOT/s6" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_contains "$out" "DEFAULT: main"

it "git-merge-state: main がまだなければ DEFAULT: none と、ルートコミットを出す"
REPO="$TEST_ROOT/s7"
mkdir -p "$REPO"
git -C "$REPO" init -q -b feat/first
echo init > "$REPO/README.md"
git -C "$REPO" add README.md
git -C "$REPO" commit -q -m "chore: init"
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/first)
assert_eq 0 $?
assert_contains "$out" "DEFAULT: none"
assert_contains "$out" "DEFAULT_WORKTREE: none"
assert_contains "$out" "ROOT_COMMITS: $(git rev-parse --short HEAD)"

# ---------------------------------------------------------------------------
# SKILL.md の手順を、リンクされた worktree をまたいで通す
# ---------------------------------------------------------------------------

it "git-merge の手順: worktree の feature ブランチを rebase → チェック → マージ → 片付けまで通せる"
REPO="$TEST_ROOT/flow/app"
make_repo "$REPO"
git -C "$REPO" worktree add -q -b feat/x "$TEST_ROOT/flow/app-feat-x"
echo "# doc" > "$TEST_ROOT/flow/app-feat-x/doc.md"
git -C "$TEST_ROOT/flow/app-feat-x" add doc.md
git -C "$TEST_ROOT/flow/app-feat-x" commit -q -m "docs: doc"
commit_file "$REPO" b.txt "feat: main moves on"
git -C "$REPO" worktree add -q -b feat/other "$TEST_ROOT/flow/app-feat-other"
cd "$TEST_ROOT/flow/app-feat-other" || exit 1
make_fake_tool markdownlint '*) exit 0 ;;'

out=$("$SCRIPT" feat/x)
assert_contains "$out" "BEHIND: 1"
git -C "$TEST_ROOT/flow/app-feat-x" rebase -q main
assert_contains "$("$SCRIPT" feat/x)" "BEHIND: 0"

out=$("$SCRIPTS_DIR/../skills/language-checks/scripts/run-checks.sh" -C "$TEST_ROOT/flow/app-feat-x" --base main)
assert_eq 0 $?
assert_eq "-- doc.md" "$(fake_log markdownlint)"

git -C "$REPO" merge -q --no-ff feat/x -m "Merge: doc"
assert_eq 0 $?
"$SCRIPTS_DIR/post-merge-cleanup.sh" feat/x >/dev/null
assert_eq 0 $?
assert_file_missing "$TEST_ROOT/flow/app-feat-x"
assert_eq "" "$(git -C "$REPO" branch --list feat/x)"
assert_eq 1 "$(git -C "$REPO" rev-list --count --merges main)"
cd "$TEST_ROOT" || exit 1

it "git-merge の手順: worktree が 1 つだけでも、取り出して rebase → チェック → main に戻してマージ → 片付けまで通せる"
REPO="$TEST_ROOT/flow1/app"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
echo "# doc" > "$REPO/doc.md"
git -C "$REPO" add doc.md
git -C "$REPO" commit -q -m "docs: doc"
git -C "$REPO" switch -q main
commit_file "$REPO" b.txt "feat: main moves on"
cd "$REPO" || exit 1

out=$("$SCRIPT" feat/x)
assert_contains "$out" "TARGET_WORKTREE: none"
assert_contains "$out" "DEFAULT_WORKTREE: $REPO"
git -C "$REPO" switch -q feat/x
git -C "$REPO" rebase -q main
"$SCRIPTS_DIR/../skills/language-checks/scripts/run-checks.sh" -C "$REPO" --base main >/dev/null
assert_eq 0 $?
git -C "$REPO" switch -q main
git -C "$REPO" merge -q --no-ff feat/x -m "Merge: doc"
assert_eq 1 "$(git -C "$REPO" rev-list --count --merges main)"
"$SCRIPTS_DIR/post-merge-cleanup.sh" feat/x >/dev/null
assert_eq "" "$(git -C "$REPO" branch --list feat/x)"
cd "$TEST_ROOT" || exit 1

finish
