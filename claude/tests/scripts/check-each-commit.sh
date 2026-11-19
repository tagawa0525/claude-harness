#!/usr/bin/env bash
# claude/scripts/check-each-commit.sh（<base>..HEAD の各コミットで検査コマンドを実行する）の配備の確認
#
# 実行: bash claude/tests/scripts/check-each-commit.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script check-each-commit（ADR-0101）。引数の読み方は crates/claude-hooks/src/script/check_each_commit.rs の
# 単体のテストが、各コミットの検査、後片付け、環境の失敗、中断は crates/claude-hooks/tests/script_check_each_commit.rs が
# 確かめ、ここでは sh からブランチでビルドしたバイナリを呼べることと、行、終了コードを確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
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

export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"

it "check-each-commit: sh から、各コミットの行を古い順に出し、失敗があれば exit 1 で、一時ディレクトリを残さない"
out=$("$SCRIPT" main -- test ! -e BAD 2>/dev/null)
assert_eq 1 $?
assert_eq 3 "$(wc -l <<<"$out" | tr -d ' ')"
assert_contains "$(sed -n 1p <<<"$out")" "$(git rev-parse --short HEAD~2) feat: first exit=0"
assert_contains "$(sed -n 2p <<<"$out")" "feat: second breaks exit=1"
assert_eq "" "$(ls -A "$TMPDIR")"

it "check-each-commit: sh から、区切りの -- が無ければ exit 2"
"$SCRIPT" main true >/dev/null 2>&1
assert_eq 2 $?

finish
