#!/usr/bin/env bash
# adr/scripts（new-adr.sh）のテスト
#
# 実行: bash .claude/tests/scripts/adr.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

NEW_ADR="$CLAUDE_DIR/skills/adr/scripts/new-adr.sh"
TODAY=$(date +%F)

# ===========================================================================
# adr/scripts/new-adr.sh
# ===========================================================================

REPO="$TEST_ROOT/adr/fresh"
make_repo "$REPO"
cd "$REPO" || exit 1

it "new-adr: ADR が無ければ docs/adr/0001-<slug>.md を作り、そのパスを出す"
out=$("$NEW_ADR" use-foo "foo を使う")
assert_eq 0 $?
assert_eq "docs/adr/0001-use-foo.md" "$out"
assert_file_exists "$REPO/docs/adr/0001-use-foo.md"

body=$(cat "$REPO/docs/adr/0001-use-foo.md")

it "new-adr: 前付けは proposed と今日の日付で、置き換えと issue のキーを空で持つ"
assert_eq "---
status: proposed
date: $TODAY
requires: []
supersedes: []
superseded-by:
issues: []
---" "$(sed -n 1,8p "$REPO/docs/adr/0001-use-foo.md")"

it "new-adr: 表題は ADR-NNNN と、渡した決定の 1 文"
assert_contains "$body" $'\n# ADR-0001: foo を使う\n'

it "new-adr: 必須の節（背景・検討した案・決定と理由）と推奨の節（帰結・確認）を順に持つ"
assert_eq $'## 背景\n## 検討した案\n## 決定と理由\n### 帰結\n### 確認' \
  "$(grep -E '^##' "$REPO/docs/adr/0001-use-foo.md")"

it "new-adr: サブディレクトリから実行してもリポジトリの直下の docs/adr に作る"
mkdir -p "$REPO/src/deep"
out=$(cd "$REPO/src/deep" && "$NEW_ADR" use-bar "bar を使う")
assert_eq "docs/adr/0002-use-bar.md" "$out"
assert_file_exists "$REPO/docs/adr/0002-use-bar.md"

REPO="$TEST_ROOT/adr/history"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p docs/adr
touch docs/adr/0001-keep-a.md docs/adr/0003-drop-c.md
git add docs && git commit -q -m "docs(adr): a and c"
git rm -q docs/adr/0003-drop-c.md && git commit -q -m "docs(adr): drop c"

it "new-adr: 番号は、履歴にだけ残る削除済みの ADR も含めた最大 + 1"
out=$("$NEW_ADR" use-d "d を使う")
assert_eq "docs/adr/0004-use-d.md" "$out"

it "new-adr: 番号は、まだコミットしていない ADR も含めた最大 + 1"
out=$("$NEW_ADR" use-e "e を使う")
assert_eq "docs/adr/0005-use-e.md" "$out"

it "new-adr: slug が英小文字の kebab-case でなければ失敗し、ファイルを作らない"
out=$("$NEW_ADR" Use_F "f を使う" 2>&1)
assert_eq 1 $?
assert_contains "$out" "kebab-case"
assert_file_missing "$REPO/docs/adr/0006-Use_F.md"

it "new-adr: 表題が無ければ使い方を示して失敗する"
out=$("$NEW_ADR" use-g 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

git switch -q -c other
touch docs/adr/0009-on-other.md
git add docs && git commit -q -m "docs(adr): on other branch"
git switch -q main

it "new-adr: 番号は、別のブランチにだけコミットされた ADR も含めた最大 + 1"
out=$("$NEW_ADR" use-h "h を使う")
assert_eq "docs/adr/0010-use-h.md" "$out"

REPO="$TEST_ROOT/adr/three-digits"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p docs/adr
touch docs/adr/001-keep-a.md docs/adr/007-keep-g.md
git add docs && git commit -q -m "docs(adr): three digits"

it "new-adr: 3 桁の番号の ADR も数え、その最大 + 1 から 4 桁で振る"
out=$("$NEW_ADR" use-i "i を使う")
assert_eq "docs/adr/0008-use-i.md" "$out"

it "new-adr: 履歴を読めなければ失敗し、ファイルを作らない"
REAL_GIT=$(command -v git)
make_fake_tool git "log*) exit 128 ;;
  *) exec \"$REAL_GIT\" \"\$@\" ;;"
"$NEW_ADR" use-j "j を使う" >/dev/null 2>&1
assert_eq 1 $(($? != 0))
remove_fake_tool git
assert_eq "" "$(find "$REPO/docs/adr" -name '*-use-j.md')"

finish
