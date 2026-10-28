#!/usr/bin/env bash
# .claude/scripts/git-merge-state.sh（/git-merge の事前確認）のテスト
#
# 実行: bash .claude/tests/scripts/git-merge-state.sh（まとめて実行: bash .claude/tests/scripts.sh）

# ADR のフロントマターの status は design-records（設計の記録の CLI）が読む。リポジトリのクレートをビルドして PATH の先頭に置く。
# lib.sh が HOME を差し替える前にビルドする（~/.cargo/config.toml の sccache 等を使うため）
DESIGN_RECORDS_DIR=$(cd "$(dirname "$0")/../../../modules/home/parts/design-records" && pwd)
# バイナリの場所は cargo に聞く（~/.cargo/config.toml の target-dir や CARGO_TARGET_DIR で target/ の外に出る）
BUILT=$(cargo build -q --message-format=json --manifest-path "$DESIGN_RECORDS_DIR/Cargo.toml") || exit 1
DESIGN_RECORDS_BIN=$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "design-records" and .executable != null) | .executable' <<<"$BUILT")
if [[ ! -x "$DESIGN_RECORDS_BIN" ]]; then
  echo "ERROR: ビルドした design-records が見つかりません: '$DESIGN_RECORDS_BIN'" >&2
  exit 1
fi
PATH="${DESIGN_RECORDS_BIN%/*}:$PATH"
export PATH

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
# PROPOSED_ADRS: ブランチの feat / fix のコミットが Refs: で挙げ、先端で proposed の ADR
# ---------------------------------------------------------------------------

# write_adr <repo> <number> <status>: docs/adr/<number>-adr.md をフロントマターの status 付きで書く
write_adr() {
  mkdir -p "$1/docs/adr"
  printf -- '---\nstatus: %s\ndate: 2026-10-29\n---\n\n# ADR-%s\n' "$3" "$2" > "$1/docs/adr/$2-adr.md"
}

# commit_refs <repo> <subject> <refs>: 本文に Refs: の行を持つコミット（変更は log.txt への追記）
commit_refs() {
  echo "$2" >> "$1/log.txt"
  git -C "$1" add log.txt
  git -C "$1" commit -q -m "$2" -m "Why." -m "Refs: $3"
}

# adr_repo <dir>: main に proposed の ADR-0001、0002、0004 と accepted の ADR-0003 を持ち、feat/x を切って main に戻したリポジトリ
adr_repo() {
  make_repo "$1"
  write_adr "$1" 0001 proposed
  write_adr "$1" 0002 proposed
  write_adr "$1" 0003 accepted
  write_adr "$1" 0004 proposed
  git -C "$1" add docs
  git -C "$1" commit -q -m "docs: seed the ADRs"
  git -C "$1" switch -q -c feat/x
}

it "git-merge-state: feat と fix のコミットが Refs: で挙げた proposed の ADR を、番号の順に PROPOSED_ADRS に出す"
REPO="$TEST_ROOT/p1"
adr_repo "$REPO"
commit_refs "$REPO" "fix(parser): b" "#0004, ADR-0002"
commit_refs "$REPO" "feat: a" "ADR-0001"
commit_refs "$REPO" "feat(cli)!: c" "ADR-0004"
commit_refs "$REPO" "feat: d" "ADR-0001"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: ADR-0001 ADR-0002 ADR-0004" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: 1 つのコミットの複数の Refs: の行と、scope の無い fix! を読む"
MULTI="$TEST_ROOT/p1m"
adr_repo "$MULTI"
echo m >> "$MULTI/log.txt"
git -C "$MULTI" add log.txt
git -C "$MULTI" commit -q -m "fix!: m" -m "Why." -m "Refs: ADR-0001" -m "Refs: #0004, ADR-0004"
git -C "$MULTI" switch -q main
out=$(cd "$MULTI" && "$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: ADR-0001 ADR-0004" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: リポジトリのサブディレクトリから実行しても PROPOSED_ADRS を出す"
out=$(cd "$REPO/docs" && "$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: ADR-0001 ADR-0002 ADR-0004" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: log.showSignature が true で、署名したコミットでも feat / fix の件名を読む"
REPO="$TEST_ROOT/p1s"
adr_repo "$REPO"
ssh-keygen -q -t ed25519 -N '' -f "$TEST_ROOT/p1s-key"
git -C "$REPO" config gpg.format ssh
git -C "$REPO" config user.signingkey "$TEST_ROOT/p1s-key"
git -C "$REPO" config commit.gpgSign true
git -C "$REPO" config log.showSignature true
commit_refs "$REPO" "feat: a" "ADR-0001"
git -C "$REPO" switch -q main
out=$(cd "$REPO" && "$SCRIPT" feat/x 2>/dev/null)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: ADR-0001" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: Refs: で挙げた ADR がブランチの先端で accepted か superseded なら出さない"
REPO="$TEST_ROOT/p2"
adr_repo "$REPO"
commit_refs "$REPO" "feat: a" "ADR-0003"
write_adr "$REPO" 0004 superseded
git -C "$REPO" add docs
commit_refs "$REPO" "fix: c" "ADR-0004"
write_adr "$REPO" 0001 accepted
git -C "$REPO" add docs
commit_refs "$REPO" "feat: b" "ADR-0001"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: none" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: feat / fix 以外のコミット、Refs: の行の外、別のリポジトリの ADR だけが挙げた proposed の ADR は出さない"
REPO="$TEST_ROOT/p3"
adr_repo "$REPO"
commit_refs "$REPO" "docs(adr): a" "ADR-0001"
commit_refs "$REPO" "test: b" "ADR-0004"
commit_refs "$REPO" "refactor: feat: c" "ADR-0002"
echo e >> "$REPO/log.txt"
git -C "$REPO" commit -q -am "feat: e" -m "ADR-0001 の案は採らない。" -m "Refs: #0004"
commit_refs "$REPO" "feat: f" "#0004, nixfiles の ADR-0002"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: none" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: main に入ったコミットの Refs: は見ない"
REPO="$TEST_ROOT/p4"
adr_repo "$REPO"
git -C "$REPO" switch -q main
commit_refs "$REPO" "feat: on main" "ADR-0001"
git -C "$REPO" switch -q feat/x
git -C "$REPO" rebase -q main
commit_refs "$REPO" "feat: a" "ADR-0004"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: ADR-0004" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: Refs: で挙げた番号の ADR のファイルが先端に無ければ出さない"
REPO="$TEST_ROOT/p7"
adr_repo "$REPO"
commit_refs "$REPO" "feat: a" "ADR-0009"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: none" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: docs/adr があって design-records が PATH に無ければ ERROR で止まる"
NODR="$TEST_ROOT/no-design-records"
mkdir -p "$NODR"
for tool in bash git sed tr grep sort awk mktemp; do ln -s "$(command -v "$tool")" "$NODR/$tool"; done
out=$(PATH="$NODR" "$SCRIPT" feat/x 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: design-records"
assert_not_contains "$out" "PROPOSED_ADRS:"

it "git-merge-state: main がまだなければ、対象の全コミットの Refs: を見る"
REPO="$TEST_ROOT/p6"
mkdir -p "$REPO"
git -C "$REPO" init -q -b feat/first
write_adr "$REPO" 0001 proposed
git -C "$REPO" add docs
commit_refs "$REPO" "feat: a" "ADR-0001"
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/first)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: ADR-0001" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: docs/adr の無いリポジトリでは PROPOSED_ADRS を出さない"
REPO="$TEST_ROOT/p5"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
commit_refs "$REPO" "feat: a" "ADR-0001"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_not_contains "$out" "PROPOSED_ADRS:"

it "git-merge-state: 件名と Refs: の間に空行の無い fix のコミットも読む"
REPO="$TEST_ROOT/p8"
adr_repo "$REPO"
echo e >> "$REPO/log.txt"
git -C "$REPO" add log.txt
git -C "$REPO" commit -q -m $'fix: e\nRefs: ADR-0004'
git -C "$REPO" switch -q main
cd "$REPO" || exit 1
out=$("$SCRIPT" feat/x)
assert_eq 0 $?
assert_eq "PROPOSED_ADRS: ADR-0004" "$(grep '^PROPOSED_ADRS:' <<<"$out")"

it "git-merge-state: docs/adr の無いリポジトリでは、design-records が PATH に無くても続行する"
out=$(cd "$TEST_ROOT/p5" && PATH="$NODR" "$SCRIPT" feat/x 2>&1)
assert_eq 0 $?
assert_not_contains "$out" "PROPOSED_ADRS:"

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
