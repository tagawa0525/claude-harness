#!/usr/bin/env bash
# claude/skills/handoff/scripts/resume-state.sh（再開時の状態の収集）の配備の確認
#
# 実行: bash claude/tests/scripts/resume-state.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script resume-state。節ごとの場合分けは crates/claude-hooks/tests/script_resume_state.rs が確かめ、
# ここではスキルの下の sh からブランチでビルドしたバイナリを呼べることと、節の見出しと、unmarked= の行の一覧のコマンドを
# 確かめる
#
# 一覧のコマンドが呼ぶ design-records は、リポジトリのクレートをビルドして PATH の先頭に置く。lib.sh が HOME を差し替える
# 前にビルドする（~/.cargo/config.toml の sccache 等を使うため）
DESIGN_RECORDS_DIR=$(cd "$(dirname "$0")/../../../crates/design-records" && pwd)
source "$(dirname "$0")/../cargo-bin.sh"
DESIGN_RECORDS_BIN=$(cargo_bin "$DESIGN_RECORDS_DIR" design-records) || exit 1
PATH="${DESIGN_RECORDS_BIN%/*}:$PATH"
export PATH
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

SCRIPT="$CLAUDE_DIR/skills/handoff/scripts/resume-state.sh"

REPO="$TEST_ROOT/resume/app"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p docs/issues
printf -- '---\nstatus: open\nblocked-by: ["#0001"]\n---\n\n# a\n' > docs/issues/0002-a.md
git add docs && git commit -q -m "docs: seed" -m "Claude-Session: sess-1"

it "resume-state: スキルの下の sh から、worktree、PR、Slurm、issue の節を出す"
out=$("$SCRIPT")
assert_eq 0 $?
assert_contains "$out" "== worktrees"
assert_contains "$out" "$REPO main session=sess-1"
assert_contains "$out" "== prs"
assert_contains "$out" "== slurm"
assert_contains "$out" "== issues"
assert_contains "$out" "docs/issues/0002-a.md blocked-by=[\"#0001\"]"

it "resume-state: unmarked= の行のコマンドは、resume-state と同じ読み方で open の issue を全部出す"
printf -- '---\nstatus: "open" # x\n---\n\n# c\n' > docs/issues/0003-c.md
printf -- '# d\n\nstatus: open\n' > docs/issues/0004-d.md
printf -- '---\nstatus: closed\n---\n\n# e\n' > docs/issues/0005-e.md
out=$("$SCRIPT")
assert_contains "$out" "unmarked=1 "
list=$(sed -n 's/^unmarked=[0-9]* [^:]*: //p' <<<"$out")
assert_eq "docs/issues/0002-a.md
docs/issues/0003-c.md" "$(bash -c "$list")"

finish
