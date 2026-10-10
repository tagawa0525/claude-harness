#!/usr/bin/env bash
# .claude/scripts/remap-commit-refs.sh（履歴の書き換えで変わったコミットの番号を、追跡している
# 文書の中で付け直す）のテスト
#
# 実行: bash .claude/tests/scripts/remap-commit-refs.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

SCRIPT="$SCRIPTS_DIR/remap-commit-refs.sh"

# 書き換えのたびに git が post-rewrite に渡す「旧 新」の対応を記録する hook。
# 実物の rebase / amend が出す入力の形をそのまま使うため
HOOKS="$TEST_ROOT/capture-hooks"
mkdir -p "$HOOKS"
cat > "$HOOKS/post-rewrite" <<'HOOK'
#!/usr/bin/env bash
cat > "$(git rev-parse --git-dir)/rewritten"
HOOK
chmod +x "$HOOKS/post-rewrite"
git config --global core.hooksPath "$HOOKS"

# new_repo <dir>: main に初期コミットを持つ repo を作り、そこへ移る
new_repo() {
  make_repo "$1"
  cd "$1" || exit 1
}

# record <file> <text>: ファイルに 1 行足してコミットする
record() {
  printf '%s\n' "$2" >> "$1"
  git add "$1"
  git commit -q -m "docs: $2"
}

it "rebase: 対応表の旧の番号を、同じ桁数の新の番号に置き換える（post-rewrite の入力の形）"
new_repo "$TEST_ROOT/rebase"
git switch -q -c feat
commit_file "$PWD" src.txt "feat: measured"
old=$(git rev-parse HEAD)
record notes.md "measured at ${old:0:8} and ${old:0:12}; full $old"
git switch -q main
commit_file "$PWD" other.txt "chore: main moves"
git switch -q feat
git rebase -q main
new=$(git rev-parse HEAD~1)
out=$("$SCRIPT" < .git/rewritten)
assert_eq 0 $?
assert_eq "measured at ${new:0:8} and ${new:0:12}; full $new" "$(cat notes.md)"
assert_contains "$out" "REPLACED: notes.md 3"
assert_contains "$out" "FILES: 1"
assert_contains "$out" "REFS: 3"

it "rebase: 置き換えた結果はコミットせず、作業ツリーの変更として残す"
assert_eq "M notes.md" "$(git status --porcelain | sed 's/^ *//')"

it "amend: 直す前のコミットの番号を、直した後の番号に置き換える（コミットしていない文書も）"
new_repo "$TEST_ROOT/amend"
commit_file "$PWD" src.txt "feat: measured"
old=$(git rev-parse HEAD)
printf 'result from %s\n' "${old:0:7}" > README.md
echo fix >> src.txt
git add src.txt
git commit -q --amend --no-edit
new=$(git rev-parse HEAD)
out=$("$SCRIPT" < .git/rewritten)
assert_eq 0 $?
assert_eq "result from ${new:0:7}" "$(cat README.md)"

it "引数のファイルからも対応を読む"
new_repo "$TEST_ROOT/arg"
commit_file "$PWD" src.txt "feat: measured"
old=$(git rev-parse HEAD)
record notes.md "at ${old:0:8}"
new=1111111111111111111111111111111111111111
printf '%s %s\n' "$old" "$new" > "$TEST_ROOT/map-arg"
out=$("$SCRIPT" "$TEST_ROOT/map-arg")
assert_eq 0 $?
assert_eq "at 11111111" "$(cat notes.md)"

it "commit-map: 見出しの行を読み飛ばし、変わらないコミットの行は置き換えない"
new_repo "$TEST_ROOT/commit-map-repo"
commit_file "$PWD" a.txt "feat: a"
a=$(git rev-parse HEAD)
commit_file "$PWD" b.txt "feat: b"
b=$(git rev-parse HEAD)
record notes.md "a ${a:0:8}, b ${b:0:8}"
a_new=2222222222222222222222222222222222222222
{
  printf 'old                                      new\n'
  printf '%s %s\n' "$a" "$a_new"
  printf '%s %s\n' "$b" "$b"
} > "$TEST_ROOT/commit-map.txt"
out=$("$SCRIPT" "$TEST_ROOT/commit-map.txt")
assert_eq 0 $?
assert_eq "a 22222222, b ${b:0:8}" "$(cat notes.md)"
assert_contains "$out" "REFS: 1"

it "commit-map: 書き換えで消えたコミット（新が 0 だけ）を文書が指していれば止め、何も変えない"
new_repo "$TEST_ROOT/pruned"
commit_file "$PWD" a.txt "feat: a"
a=$(git rev-parse HEAD)
commit_file "$PWD" b.txt "feat: b"
b=$(git rev-parse HEAD)
record notes.md "a ${a:0:8}, b ${b:0:8}"
{
  printf 'old                                      new\n'
  printf '%s %s\n' "$a" 0000000000000000000000000000000000000000
  printf '%s %s\n' "$b" 3333333333333333333333333333333333333333
} > "$TEST_ROOT/pruned-map"
out=$("$SCRIPT" "$TEST_ROOT/pruned-map" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_contains "$out" "${a:0:8}"
assert_eq "a ${a:0:8}, b ${b:0:8}" "$(cat notes.md)"

it "曖昧（同じ短い番号が複数の旧に当たる）なら止め、どのファイルも変えない"
new_repo "$TEST_ROOT/ambiguous"
record ok.md "unique abcdef0123"
record notes.md "short abcdef0"
{
  printf '%s %s\n' abcdef0123456789abcdef0123456789abcdef01 4444444444444444444444444444444444444444
  printf '%s %s\n' abcdef0fedcba9876543210fedcba9876543210f 5555555555555555555555555555555555555555
} > "$TEST_ROOT/ambiguous-map"
out=$("$SCRIPT" "$TEST_ROOT/ambiguous-map" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_contains "$out" "abcdef0"
assert_eq "short abcdef0" "$(cat notes.md)"
assert_eq "unique abcdef0123" "$(cat ok.md)"

it "同じ旧に別々の新が対応する入力は止める"
printf '%s %s\n%s %s\n' \
  abcdef0123456789abcdef0123456789abcdef01 4444444444444444444444444444444444444444 \
  abcdef0123456789abcdef0123456789abcdef01 5555555555555555555555555555555555555555 \
  > "$TEST_ROOT/conflict-map"
out=$("$SCRIPT" "$TEST_ROOT/conflict-map" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "完全な番号でない行は止める"
printf 'abcdef0 4444444\n' > "$TEST_ROOT/bad-map"
out=$("$SCRIPT" "$TEST_ROOT/bad-map" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "追跡していないファイル、対応表にない 16 進の語、単語の一部、7 桁未満は変えない"
new_repo "$TEST_ROOT/untouched"
commit_file "$PWD" src.txt "feat: measured"
old=$(git rev-parse HEAD)
record notes.md "at ${old:0:8}"
line="x${old:0:8} ${old:0:8}g ${old:0:6} deadbeef ${old:0:8}_1 cafebabe12"
record other.md "$line"
printf 'untracked %s\n' "${old:0:8}" > untracked.md
new=6666666666666666666666666666666666666666
printf '%s %s\n' "$old" "$new" > "$TEST_ROOT/untouched-map"
out=$("$SCRIPT" "$TEST_ROOT/untouched-map")
assert_eq 0 $?
assert_eq "at 66666666" "$(cat notes.md)"
assert_eq "$line" "$(cat other.md)"
assert_eq "untracked ${old:0:8}" "$(cat untracked.md)"
assert_not_contains "$out" "other.md"

it "バイナリのファイルは変えない"
printf 'bin\0%s\n' "${old:0:8}" > blob.bin
git add blob.bin
git commit -q -m "chore: binary"
before=$(od -c blob.bin)
"$SCRIPT" "$TEST_ROOT/untouched-map" >/dev/null
assert_eq "$before" "$(od -c blob.bin)"

it "置き換えるものがなければ 0 件と出して成功する"
new_repo "$TEST_ROOT/none"
printf '%s %s\n' abcdef0123456789abcdef0123456789abcdef01 4444444444444444444444444444444444444444 \
  > "$TEST_ROOT/none-map"
out=$("$SCRIPT" "$TEST_ROOT/none-map")
assert_eq 0 $?
assert_contains "$out" "FILES: 0"
assert_contains "$out" "REFS: 0"

it "サブディレクトリから呼んでも、作業ツリー全体を対象にする"
new_repo "$TEST_ROOT/subdir"
commit_file "$PWD" src.txt "feat: measured"
old=$(git rev-parse HEAD)
mkdir -p docs/issues
record docs/issues/x.md "at ${old:0:8}"
printf '%s %s\n' "$old" 7777777777777777777777777777777777777777 > "$TEST_ROOT/subdir-map"
out=$(cd docs && "$SCRIPT" "$TEST_ROOT/subdir-map")
assert_eq 0 $?
assert_eq "at 77777777" "$(cat docs/issues/x.md)"
assert_contains "$out" "REPLACED: docs/issues/x.md 1"

finish
