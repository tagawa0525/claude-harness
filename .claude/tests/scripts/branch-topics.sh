#!/usr/bin/env bash
# .claude/scripts/branch-topics.sh（ブランチが含むトピックの要約）のテスト
#
# 実行: bash .claude/tests/scripts/branch-topics.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

# write_front <path> <status>: 前付けに status を持つ文書を書く
write_front() {
  mkdir -p "$(dirname "$1")"
  printf -- '---\nstatus: %s\ndate: 2026-10-09\n---\n\n# %s\n' "$2" "$(basename "$1")" > "$1"
}

REPO="$TEST_ROOT/topics/app"
make_repo "$REPO"
cd "$REPO" || exit 1
write_front docs/adr/0001-keep-a.md accepted
write_front docs/adr/0002-keep-b.md accepted
cp docs/adr/0001-keep-a.md docs/adr/README.md
write_front docs/issues/0001-bug.md open
git add docs && git commit -q -m "docs: seed"

git switch -q -c feat/x
write_front docs/adr/0003-use-c.md proposed
write_front docs/adr/0004-use-d.md accepted
sed -i 's/^status: accepted$/status: superseded/' docs/adr/0001-keep-a.md
echo "body" >> docs/adr/0002-keep-b.md
write_front docs/issues/0002-todo.md open
sed -i 's/^status: open$/status: closed/' docs/issues/0001-bug.md
echo "readme" >> docs/adr/README.md
git add docs && git commit -q -m "docs(adr): decide c and d"
commit_file "$REPO" crates/xs/src/lib.rs "feat(xs): add x"
commit_file "$REPO" crates/xs/tests/x.rs "test(xs): cover x"
commit_file "$REPO" crates/lattice/src/lib.rs "feat(lattice): add y"
commit_file "$REPO" top.txt "Update top"

it "branch-topics: 起点と、型ごとのコミット数を出す（件数の多い順、同数は名前順。型のない件名は other）"
out=$("$SCRIPTS_DIR/branch-topics.sh" main)
assert_eq 0 $?
assert_contains "$out" "BASE: main"
assert_contains "$out" "COMMITS: 5"
assert_contains "$out" "TYPES: feat=2 docs=1 other=1 test=1"

it "branch-topics: 新しい ADR を status 付きで 1 行ずつ出す（README は ADR ではない）"
assert_contains "$out" "ADR_NEW: docs/adr/0003-use-c.md (proposed)"
assert_contains "$out" "ADR_NEW: docs/adr/0004-use-d.md (accepted)"
assert_not_contains "$out" "README"

it "branch-topics: 既存の ADR の status の変化を出す（本文だけの変更は出さない）"
assert_contains "$out" "ADR_STATUS: docs/adr/0001-keep-a.md accepted -> superseded"
assert_not_contains "$out" "0002-keep-b"

it "branch-topics: 新しい issue と status の変化（閉じたもの）を出す"
assert_contains "$out" "ISSUE_NEW: docs/issues/0002-todo.md (open)"
assert_contains "$out" "ISSUE_STATUS: docs/issues/0001-bug.md open -> closed"

it "branch-topics: 触れたディレクトリをパスの先頭 2 段で出す（直下のファイルは .）"
assert_contains "$out" "DIRS: . crates/lattice crates/xs docs/adr docs/issues"

it "branch-topics: 起点を省くと既定ブランチ（リモートが無ければ main）と比べる"
out=$("$SCRIPTS_DIR/branch-topics.sh")
assert_contains "$out" "BASE: main"
assert_contains "$out" "COMMITS: 5"

it "branch-topics: 起点の後に main が進んでも、ブランチで起きた変更だけを出す（merge-base と比べる）"
git switch -q main
write_front docs/adr/0005-on-main.md accepted
git add docs && git commit -q -m "docs(adr): on main"
git switch -q feat/x
out=$("$SCRIPTS_DIR/branch-topics.sh" main)
assert_not_contains "$out" "0005-on-main"
assert_contains "$out" "COMMITS: 5"

it "branch-topics: 起点の ref が無ければエラー"
"$SCRIPTS_DIR/branch-topics.sh" no-such-ref >/dev/null 2>&1
assert_eq 1 $?

PLAIN="$TEST_ROOT/topics/plain"
make_repo "$PLAIN"
cd "$PLAIN" || exit 1
git switch -q -c fix/y
commit_file "$PLAIN" src/a.rs "fix: a"

it "branch-topics: docs/adr・docs/issues が無いリポジトリでは ADR / issue の行を出さずに成功する"
out=$("$SCRIPTS_DIR/branch-topics.sh" main)
assert_eq 0 $?
assert_contains "$out" "TYPES: fix=1"
assert_contains "$out" "DIRS: src"
assert_not_contains "$out" "ADR_"
assert_not_contains "$out" "ISSUE_"

finish
