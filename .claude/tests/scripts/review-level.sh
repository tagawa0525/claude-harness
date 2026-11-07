#!/usr/bin/env bash
# .claude/scripts/review-level.sh（ローカルの /code-review のレベルと対象を決める）のテスト
#
# 実行: bash .claude/tests/scripts/review-level.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

REPO="$TEST_ROOT/rl/app"
make_repo "$REPO"
cd "$REPO" || exit 1

it "review-level: docs/ の ADR でない文書だけの変更は medium で 1 周"
git switch -q -c feat/docs
commit_file "$REPO" docs/plans/x.md "docs: plan"
commit_file "$REPO" docs/issues/y.md "docs: issue"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_eq 0 $?
assert_contains "$out" "LEVEL: medium"
assert_contains "$out" "TARGET: feat/docs"
assert_contains "$out" "BASE: main"
assert_contains "$out" "FILES: 2"
assert_contains "$out" "ROUNDS: 1"

it "review-level: 文書でないファイルを含む変更は high で 3 周"
git switch -q main
git switch -q -c feat/mixed
commit_file "$REPO" docs/plans/x.md "docs: plan"
commit_file "$REPO" .claude/scripts/x.sh "feat: x"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "LEVEL: high"
assert_contains "$out" "ROUNDS: 3"

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

it "review-level: WORKTREE は対象のブランチをチェックアウトしている worktree のパス（/code-review に渡す）"
git switch -q main
git worktree add -q "$TEST_ROOT/rl/app-stack" feat/stack
out=$("$SCRIPTS_DIR/review-level.sh" feat/stack --base feat/docs)
assert_contains "$out" "WORKTREE: $TEST_ROOT/rl/app-stack"

it "review-level: 引数なしなら WORKTREE は cwd の worktree"
out=$(cd "$TEST_ROOT/rl/app-stack" && "$SCRIPTS_DIR/review-level.sh" --base feat/docs)
assert_contains "$out" "WORKTREE: $TEST_ROOT/rl/app-stack"

it "review-level: どの worktree にもチェックアウトされていないブランチは WORKTREE: none"
out=$("$SCRIPTS_DIR/review-level.sh" feat/docs)
assert_contains "$out" "WORKTREE: none"

it "review-level: worktree のディレクトリが消えている（prunable）ブランチは WORKTREE: none"
git worktree add -q "$TEST_ROOT/rl/app-gone" -b feat/gone feat/docs
commit_file "$TEST_ROOT/rl/app-gone" docs/plans/gone.md "docs: gone"
rm -rf "$TEST_ROOT/rl/app-gone"
out=$("$SCRIPTS_DIR/review-level.sh" feat/gone)
assert_contains "$out" "WORKTREE: none"

# ローカルの main が origin/main より古いとき（他の PR がマージされた直後など）、main を比較元にすると
# origin/main 側の変更が差分に混ざる。新しい方を比較元にする
STALE="$TEST_ROOT/rl/stale"
make_repo "$STALE"
make_remote "$STALE"
cd "$STALE" || exit 1

it "review-level: ローカルの main が origin/main より古ければ origin/main を比較元にする"
git switch -q -c feat/doc
commit_file "$STALE" docs/plans/x.md "docs: plan"
git switch -q main
commit_file "$STALE" src/other.rs "feat: other pr"
git push -q origin main
git reset -q --hard HEAD~1
git rebase -q origin/main feat/doc
out=$("$SCRIPTS_DIR/review-level.sh" feat/doc)
assert_contains "$out" "BASE: origin/main"
assert_contains "$out" "FILES: 1"
assert_contains "$out" "LEVEL: medium"

it "review-level: ローカルの main が origin/main より進んでいれば main を比較元にする（git-merge の流れ）"
git switch -q main
git merge -q --ff-only origin/main
commit_file "$STALE" src/unpushed.rs "feat: unpushed"
git switch -q -c feat/doc2
commit_file "$STALE" docs/plans/y.md "docs: plan2"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "BASE: main"
assert_contains "$out" "FILES: 1"
assert_contains "$out" "LEVEL: medium"

# 既定ブランチが main でないリポジトリ（master など）でも使える。
# 既定ブランチの探し方は worktree-add.sh / branch-topics.sh と同じ順
MASTER="$TEST_ROOT/rl/master-repo"
mkdir -p "$MASTER"
git -C "$MASTER" init -q -b master
echo init > "$MASTER/README.md"
git -C "$MASTER" add README.md
git -C "$MASTER" commit -q -m "chore: init"
cd "$MASTER" || exit 1

it "review-level: 既定ブランチが master なら master を比較元にする"
git switch -q -c feat/doc
commit_file "$MASTER" docs/plans/x.md "docs: plan"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_eq 0 $?
assert_contains "$out" "BASE: master"
assert_contains "$out" "LEVEL: medium"

it "review-level: master のローカルが origin/master より古ければ origin/master を比較元にする"
git init -q --bare "$MASTER.git"
git remote add origin "$MASTER.git"
git push -q origin master
git switch -q master
commit_file "$MASTER" src/other.rs "feat: other pr"
git push -q origin master
git reset -q --hard HEAD~1
git rebase -q origin/master feat/doc
out=$("$SCRIPTS_DIR/review-level.sh" feat/doc)
assert_contains "$out" "BASE: origin/master"
assert_contains "$out" "FILES: 1"
assert_contains "$out" "LEVEL: medium"

it "review-level: origin/HEAD が指すブランチを既定ブランチにする"
git remote set-head origin master
out=$("$SCRIPTS_DIR/review-level.sh" feat/doc)
assert_contains "$out" "BASE: origin/master"

# 周の上限（ROUNDS）は、挙動を変えない文書と規則の文書だけのブランチで 1、ほかは 3。LEVEL も同じ分け方
ROUNDS_REPO="$TEST_ROOT/rl/rounds"
make_repo "$ROUNDS_REPO"
cd "$ROUNDS_REPO" || exit 1
mkdir -p docs/adr
cat >docs/adr/0001-x.md <<'ADR'
---
status: accepted
---

# ADR-0001: x

## 決定と理由

x にする。

## 変更

- 2026-01-01: 最初の変更
ADR
cat >docs/adr/0002-y.md <<'ADR'
---
status: accepted
---

# ADR-0002: y

## 決定と理由

y にする。
ADR
git add docs/adr
git commit -q -m "docs: adr"

it "review-level: スキルの文書と CLAUDE.md だけの変更は medium で 1 周"
git switch -q -c feat/rule-docs main
commit_file "$ROUNDS_REPO" .claude/skills/x/SKILL.md "docs(skills): x"
commit_file "$ROUNDS_REPO" CLAUDE.md "docs: claude"
commit_file "$ROUNDS_REPO" .claude/README.md "docs: readme"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 1"
assert_contains "$out" "LEVEL: medium"

it "review-level: 規則の文書の判定は check-merge-review と同じ（どこかの .claude の下、SKILL.md）"
git switch -q -c feat/rule-docs-elsewhere main
commit_file "$ROUNDS_REPO" skills/b/SKILL.md "docs(skills): b"
commit_file "$ROUNDS_REPO" sub/.claude/c.md "docs: c"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 1"

it "review-level: スキルの中のスクリプトを含む変更は high で 3 周"
git switch -q -c feat/skill-script main
commit_file "$ROUNDS_REPO" .claude/skills/x/SKILL.md "docs(skills): x"
commit_file "$ROUNDS_REPO" .claude/skills/x/scripts/a.sh "feat: a"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 3"
assert_contains "$out" "LEVEL: high"

it "review-level: docs/ の外の .md（検証の記録など）は 3 周"
git switch -q -c feat/record main
commit_file "$ROUNDS_REPO" examples/p/verification.md "docs: record"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 3"
assert_contains "$out" "LEVEL: high"

it "review-level: 新しい ADR は 3 周"
git switch -q -c feat/new-adr main
commit_file "$ROUNDS_REPO" docs/adr/0003-z.md "docs: adr"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 3"
assert_contains "$out" "LEVEL: high"

it "review-level: 既存の ADR の「## 変更」の節への追記だけなら 1 周"
git switch -q -c feat/adr-change main
echo "- 2026-02-01: 次の変更" >>docs/adr/0001-x.md
git commit -q -am "docs(adr): change"
commit_file "$ROUNDS_REPO" docs/issues/i.md "docs: issue"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 1"
assert_contains "$out" "LEVEL: medium"

it "review-level: 「## 変更」の節の中の行を書き換えるだけなら 1 周"
git switch -q -c feat/adr-change-edit main
sed -i 's/最初の変更/最初の変更（直した）/' docs/adr/0001-x.md
git commit -q -am "docs(adr): fix change"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 1"

it "review-level: ADR の決定の節も変えるなら 3 周"
git switch -q -c feat/adr-decision main
sed -i 's/x にする。/x と w にする。/' docs/adr/0001-x.md
echo "- 2026-02-01: w を足した" >>docs/adr/0001-x.md
git commit -q -am "docs(adr): decision"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 3"
assert_contains "$out" "LEVEL: high"

it "review-level: 「## 変更」の節の無い ADR に節を足すなら 3 周"
git switch -q -c feat/adr-new-change main
printf '\n## 変更\n\n- 2026-02-01: 変えた\n' >>docs/adr/0002-y.md
git commit -q -am "docs(adr): add change"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 3"

it "review-level: ADR を消すなら 3 周"
git switch -q -c feat/adr-delete main
git rm -q docs/adr/0002-y.md
git commit -q -m "docs(adr): remove"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 3"

it "review-level: docs/ の下でも .md でないファイル（パッチ、ハッシュの一覧）は 3 周"
git switch -q -c feat/docs-patch main
commit_file "$ROUNDS_REPO" docs/patches/x.patch "fix: patch"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 3"
assert_contains "$out" "LEVEL: high"

it "review-level: docs/adr の ADR でない文書（README.md）は 1 周"
git switch -q -c feat/adr-readme main
commit_file "$ROUNDS_REPO" docs/adr/README.md "docs: adr readme"
out=$("$SCRIPTS_DIR/review-level.sh")
assert_contains "$out" "ROUNDS: 1"

finish
