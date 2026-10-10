#!/usr/bin/env bash
# .claude/scripts/review-level.sh（ローカルの /code-review のレベルと対象を決める）のテスト
#
# 実行: bash .claude/tests/scripts/review-level.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

REPO="$TEST_ROOT/rl/app"
make_repo "$REPO"
cd "$REPO" || exit 1

it "review-level: docs/ だけの変更は medium"
git switch -q -c feat/docs
commit_file "$REPO" docs/adr/0001-x.md "docs: adr"
commit_file "$REPO" docs/issues/y.md "docs: issue"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_eq 0 $?
assert_contains "$out" "LEVEL: medium"
assert_contains "$out" "TARGET: feat/docs"
assert_contains "$out" "BASE: main"
assert_contains "$out" "FILES: 2"

it "review-level: docs/ 以外を含む変更は high"
git switch -q main
git switch -q -c feat/mixed
commit_file "$REPO" docs/adr/0001-x.md "docs: adr"
commit_file "$REPO" .claude/skills/x/SKILL.md "docs(skills): x"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "LEVEL: high"

it "review-level: docs/ に似た別の場所（docs.md、docs-extra/、src/docs/）は文書として扱わず high"
git switch -q main
git switch -q -c feat/lookalike
commit_file "$REPO" docs.md "docs: a"
commit_file "$REPO" docs-extra/x.md "docs: b"
commit_file "$REPO" src/docs/y.md "docs: c"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "LEVEL: high"

it "review-level: src/ から docs/ への移動は旧パスも数えて high"
git switch -q main
commit_file "$REPO" src/foo.rs "feat: foo"
git switch -q -c feat/move
mkdir -p docs
git mv src/foo.rs docs/foo.rs.md
git commit -q -m "chore: move"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "LEVEL: high"

it "review-level: 差分が空なら理由を出して exit 1（別の worktree で動いている可能性）"
git switch -q main
out=$("$SCRIPTS_DIR/review-level.sh" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_contains "$out" "差分が空"

it "review-level: ブランチ名を渡せば、別の worktree にいてもそのブランチの差分で判定する"
git switch -q main
out=$("$SCRIPTS_DIR/review-level.sh" feat/docs)
assert_eq 0 $?
assert_contains "$out" "TARGET: feat/docs"
assert_contains "$out" "LEVEL: medium"

it "review-level: 存在しないブランチは exit 1"
out=$("$SCRIPTS_DIR/review-level.sh" no/such 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "review-level: --base で比較元を変えられる（積み重なったブランチ）"
git switch -q feat/docs
git switch -q -c feat/stack
commit_file "$REPO" src/z.rs "feat: z"
out=$("$SCRIPTS_DIR/review-level.sh" --base feat/docs)
assert_contains "$out" "BASE: feat/docs"
assert_contains "$out" "FILES: 1"
assert_contains "$out" "LEVEL: high"

finish
