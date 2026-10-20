#!/usr/bin/env bash
# .claude/scripts/base-moved.sh（比較元が分岐点から進み、ブランチと同じパスを触っていないかの検出）のテスト
#
# 実行: bash .claude/tests/scripts/base-moved.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

REPO="$TEST_ROOT/bm/app"
make_repo "$REPO"
make_remote "$REPO" github
cd "$REPO" || exit 1

# 別のクローンから origin/main にコミットを足す。手元の origin/main は古いまま
push_from_other() {
  local other="$REPO.other"
  [[ -d "$other" ]] || git clone -q "$REPO.git" "$other"
  git -C "$other" pull -q
  commit_file "$other" "$1" "feat: $1" "${3:-1}"
  [[ -z "${2:-}" ]] || commit_file "$other" "$2" "feat: $2"
  git -C "$other" push -q origin main
}

git switch -q -c feat/work
commit_file "$REPO" src/a.txt "feat: a"
commit_file "$REPO" src/b.txt "feat: b"

it "base-moved: base が進んでいなければ何も出さず exit 0"
out=$("$SCRIPTS_DIR/base-moved.sh")
assert_eq 0 $?
assert_eq "" "$out"

it "base-moved: base が進んでも、触ったパスが重ならなければ何も出さない"
push_from_other other/x.txt
out=$("$SCRIPTS_DIR/base-moved.sh")
assert_eq "" "$out"

it "base-moved: fetch して、進んだ件数・先頭の hash・共通のパスを出す（exit 0）"
push_from_other src/a.txt src/c.txt 5
HEAD_HASH=$(git -C "$REPO.git" rev-parse --short main)
out=$("$SCRIPTS_DIR/base-moved.sh")
assert_eq 0 $?
assert_contains "$out" "BASE_MOVED: origin/main"
assert_contains "$out" "3 commits"
assert_contains "$out" "$HEAD_HASH"
assert_contains "$out" "COMMON: src/a.txt"
assert_not_contains "$out" "src/b.txt"
assert_not_contains "$out" "src/c.txt"

it "base-moved: --base で比較元を指定できる"
git branch -q stack-base origin/main~1
out=$("$SCRIPTS_DIR/base-moved.sh" --base stack-base)
assert_contains "$out" "BASE_MOVED: stack-base"
assert_contains "$out" "2 commits"

it "base-moved: GitHub リモートがなければ何も出さない"
NOGH="$TEST_ROOT/bm/nogh"
make_repo "$NOGH"
git -C "$NOGH" switch -q -c feat/x
commit_file "$NOGH" a.txt "feat: a"
out=$(cd "$NOGH" && "$SCRIPTS_DIR/base-moved.sh")
assert_eq 0 $?
assert_eq "" "$out"

it "base-moved: fetch に失敗したら警告して、手元の追跡ブランチで比べる"
git remote set-url origin "$TEST_ROOT/bm/no-such.git"
out=$("$SCRIPTS_DIR/base-moved.sh" 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN: fetch に失敗"

finish
