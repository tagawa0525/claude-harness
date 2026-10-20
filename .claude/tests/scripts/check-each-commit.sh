#!/usr/bin/env bash
# .claude/scripts/check-each-commit.sh（<base>..HEAD の各コミットで検査コマンドを走らせる）のテスト
#
# 実行: bash .claude/tests/scripts/check-each-commit.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

SCRIPT="$SCRIPTS_DIR/check-each-commit.sh"
REPO="$TEST_ROOT/cec/app"
make_repo "$REPO"
cd "$REPO" || exit 1
git switch -q -c feat/x
commit_file "$REPO" a.txt "feat: first"
commit_file "$REPO" BAD "feat: second breaks"
git rm -q BAD
git commit -q -m "fix: third repairs"
HEAD_BEFORE=$(git rev-parse HEAD)
echo dirty > "$REPO/untracked.txt"
echo changed >> "$REPO/README.md"

# 一時ディレクトリの置き場を TEST_ROOT の中に固定して、残りを調べる
export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"

it "check-each-commit: 失敗したコミットだけ exit=1 で出て、全体は非 0 で、全コミットを走らせる"
out=$("$SCRIPT" main -- test ! -e BAD 2>/dev/null)
rc=$?
assert_eq 1 "$rc"
assert_eq 3 "$(wc -l <<<"$out" | tr -d ' ')"
assert_contains "$(sed -n 1p <<<"$out")" "feat: first exit=0"
assert_contains "$(sed -n 2p <<<"$out")" "feat: second breaks exit=1"
assert_contains "$(sed -n 3p <<<"$out")" "fix: third repairs exit=0"

it "check-each-commit: 古い順に出す"
assert_contains "$(sed -n 1p <<<"$out")" "$(git rev-parse --short "$HEAD_BEFORE~2")"

it "check-each-commit: すべて通れば exit 0"
"$SCRIPT" main -- test -e a.txt >/dev/null 2>&1
assert_eq 0 $?

it "check-each-commit: 現在の作業ツリーと HEAD に触らない"
assert_eq "$HEAD_BEFORE" "$(git rev-parse HEAD)"
assert_eq "dirty" "$(cat "$REPO/untracked.txt")"
assert_contains "$(git status --porcelain)" " M README.md"

it "check-each-commit: 一時ディレクトリと worktree の登録が残らない"
assert_eq "" "$(ls -A "$TMPDIR")"
assert_eq 1 "$(git worktree list | wc -l | tr -d ' ')"

it "check-each-commit: コマンドが異常終了しても一時ディレクトリを片付ける"
"$SCRIPT" main -- sh -c 'kill -9 $$' >/dev/null 2>&1
assert_ne 0 $?
assert_eq "" "$(ls -A "$TMPDIR")"

it "check-each-commit: 区切りの -- が無ければ exit 2"
"$SCRIPT" main true >/dev/null 2>&1
assert_eq 2 $?

it "check-each-commit: base が解決できなければ exit 2"
"$SCRIPT" nosuchref -- true >/dev/null 2>&1
assert_eq 2 $?

finish
