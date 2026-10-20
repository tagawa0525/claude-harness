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

it "check-each-commit: 検査の副産物は次のコミットに残らない"
out=$("$SCRIPT" main -- sh -c 'test ! -e artefact && touch artefact' 2>/dev/null)
assert_eq 0 $?
assert_not_contains "$out" "exit=1"

it "check-each-commit: 区切りの -- が無ければ exit 2"
"$SCRIPT" main true >/dev/null 2>&1
assert_eq 2 $?

it "check-each-commit: base が解決できなければ exit 2"
"$SCRIPT" nosuchref -- true >/dev/null 2>&1
assert_eq 2 $?

it "check-each-commit: 書き込み不可のファイルを残す検査でも一時ディレクトリが残らない"
"$SCRIPT" main -- sh -c 'mkdir d && touch d/f && chmod -R a-w d' >/dev/null 2>&1
assert_eq 0 $?
assert_eq "" "$(ls -A "$TMPDIR")"

it "check-each-commit: 検査コマンドは標準入力を継承しない（入力を待たずに終わる）"
sleep 4 | timeout 2 "$SCRIPT" main -- cat >/dev/null 2>&1
assert_eq 0 $?

it "check-each-commit: rev-list が失敗したら原因を出して exit 3（使い方の誤りの 2 と区別する）"
real_git=$(command -v git)
mkdir -p "$TEST_ROOT/failbin"
printf '#!/usr/bin/env bash\n[[ "$1" == rev-list ]] && { echo "fatal: bad object" >&2; exit 128; }\nexec %s "$@"\n' "$real_git" > "$TEST_ROOT/failbin/git"
chmod +x "$TEST_ROOT/failbin/git"
err=$(PATH="$TEST_ROOT/failbin:$PATH" "$SCRIPT" main -- true 2>&1 >/dev/null)
assert_eq 3 $?
assert_contains "$err" "git rev-list に失敗"

# FAIL_GIT_SUB に指定した git のサブコマンドだけ失敗する git を PATH の先頭に置く
printf '#!/usr/bin/env bash\nfor a in "$@"; do [[ "$a" == "${FAIL_GIT_SUB-}" ]] && { echo "fatal: injected" >&2; exit 128; }; done\nexec %s "$@"\n' "$real_git" > "$TEST_ROOT/failbin/git"

it "check-each-commit: mktemp が失敗したら exit 3 で、ルート直下に何も作らない"
TMPDIR=/nonexistent "$SCRIPT" main -- true >/dev/null 2>&1
assert_eq 3 $?
assert_file_missing /wt

it "check-each-commit: git checkout が失敗したら exit 3"
out=$(FAIL_GIT_SUB=checkout PATH="$TEST_ROOT/failbin:$PATH" "$SCRIPT" main -- true 2>/dev/null)
assert_eq 3 $?
assert_eq "" "$out"
assert_eq "" "$(ls -A "$TMPDIR")"

it "check-each-commit: git clean が失敗したらそのコミットの検査をせず exit 3"
out=$(FAIL_GIT_SUB=clean PATH="$TEST_ROOT/failbin:$PATH" "$SCRIPT" main -- true 2>/dev/null)
assert_eq 3 $?
assert_eq "" "$out"

it "check-each-commit: git worktree add が失敗したら exit 3"
FAIL_GIT_SUB=add PATH="$TEST_ROOT/failbin:$PATH" "$SCRIPT" main -- true >/dev/null 2>&1
assert_eq 3 $?

it "check-each-commit: GIT_DIR と GIT_WORK_TREE が別のリポジトリを指していても、そこに触らず対象を検査する"
OTHER="$TEST_ROOT/cec/other"
make_repo "$OTHER"
echo keep > "$OTHER/untracked.txt"
echo edit >> "$OTHER/README.md"
other_head=$(git -C "$OTHER" rev-parse HEAD)
other_status=$(git -C "$OTHER" status --porcelain)
out=$(GIT_DIR="$OTHER/.git" GIT_WORK_TREE="$OTHER" GIT_INDEX_FILE="$OTHER/.git/index" "$SCRIPT" main -- test -e a.txt 2>/dev/null)
assert_contains "$out" "feat: first exit=0"
assert_contains "$out" "fix: third repairs exit=0"
assert_eq "$other_head" "$(git -C "$OTHER" rev-parse HEAD)"
assert_eq "$other_status" "$(git -C "$OTHER" status --porcelain)"
assert_eq "keep" "$(cat "$OTHER/untracked.txt")"

finish
