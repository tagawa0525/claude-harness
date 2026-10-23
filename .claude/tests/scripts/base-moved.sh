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

it "base-moved: GitHub リモートが無くても --base を渡せば、fetch せずに手元のブランチで比べる"
git -C "$NOGH" switch -q main
commit_file "$NOGH" a.txt "feat: a on main"
commit_file "$NOGH" other.txt "feat: other"
git -C "$NOGH" switch -q feat/x
out=$(cd "$NOGH" && "$SCRIPTS_DIR/base-moved.sh" --base main 2>&1)
assert_eq 0 $?
assert_contains "$out" "BASE_MOVED: main"
assert_contains "$out" "2 commits"
assert_contains "$out" "COMMON: a.txt"
assert_not_contains "$out" "other.txt"
assert_not_contains "$out" "WARN"

it "base-moved: GitHub でない origin の追跡ブランチを --base に渡すと、fetch せずに比べて WARN で未確認と知らせる"
LOC="$TEST_ROOT/bm/local_origin"
make_repo "$LOC"
make_remote "$LOC"
git -C "$LOC" switch -q -c feat/x
commit_file "$LOC" a.txt "feat: a"
git clone -q "$LOC.git" "$LOC.other"
commit_file "$LOC.other" a.txt "feat: a on main"
git -C "$LOC.other" push -q origin main
out=$(cd "$LOC" && "$SCRIPTS_DIR/base-moved.sh" --base origin/main 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "未確認"
assert_not_contains "$out" "COMMON: a.txt"

it "base-moved: --base がローカルのブランチなら、名前の先頭がリモート名と同じでも fetch しない"
git -C "$NOGH" remote add feat "$TEST_ROOT/bm/missing-feat.git"
git -C "$NOGH" branch -q feat/parent main
out=$(cd "$NOGH" && "$SCRIPTS_DIR/base-moved.sh" --base feat/parent 2>&1)
assert_eq 0 $?
assert_contains "$out" "COMMON: a.txt"
assert_not_contains "$out" "WARN"
git -C "$NOGH" remote remove feat

it "base-moved: fetch に失敗したら警告して、手元の追跡ブランチで比べる"
git remote set-url origin "$TEST_ROOT/bm/no-such.git"
out=$("$SCRIPTS_DIR/base-moved.sh" 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN: fetch に失敗"

# --- 履歴の形ごとのフィクスチャ（それぞれ新しいリポジトリ） ---
# fixture <名前>: GitHub リモートつきのリポジトリ。ブランチ feat/work は main から切る
fixture() {
  local dir="$TEST_ROOT/bm/$1"
  make_repo "$dir"
  make_remote "$dir" github
  git -C "$dir" switch -q -c feat/work
  echo "$dir"
}
# in_other <dir> <コマンド...>: 別のクローンで実行して origin/main に push する
in_other() {
  local dir="$1" other="$1.other"
  git clone -q "$dir.git" "$other"
  shift
  (cd "$other" && "$@" && git push -q origin main)
}

it "base-moved: --base が origin 以外のリモートの追跡ブランチなら、fetch せずに比べて WARN で未確認と知らせる"
D=$(fixture base_other_remote)
commit_file "$D" src/a.txt "feat: a"
git init -q --bare "$D.up.git"
git -C "$D" remote add up "$D.up.git"
git -C "$D" push -q up main
git -C "$D" fetch -q up
git clone -q "$D.up.git" "$D.up.other"
commit_file "$D.up.other" src/a.txt "feat: up a" 3
git -C "$D.up.other" push -q origin main
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" --base up/main 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "未確認"
assert_not_contains "$out" "COMMON: src/a.txt"

it "base-moved: --base を refs/remotes/origin/<ブランチ> や裸の origin の形で渡しても、origin を fetch する"
D=$(fixture base_full_ref)
commit_file "$D" src/a.txt "feat: a"
in_other "$D" bash -c 'mkdir -p src && seq 5 > src/a.txt && git add src/a.txt && git commit -q -m "feat: other a"'
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" --base refs/remotes/origin/main 2>&1)
assert_eq 0 $?
assert_contains "$out" "COMMON: src/a.txt"
git -C "$D.other" pull -q
commit_file "$D.other" src/a.txt "feat: other a again" 6
git -C "$D.other" push -q origin main
NEW_TIP=$(git -C "$D.other" rev-parse --short HEAD)
git -C "$D" remote set-head origin main
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" --base origin 2>&1)
assert_eq 0 $?
assert_contains "$out" "$NEW_TIP"

it "base-moved: まだ fetch していない追跡ブランチを --base に渡しても、そのリモートを fetch してから比べる"
D=$(fixture base_not_fetched)
commit_file "$D" src/a.txt "feat: a"
in_other "$D" bash -c 'git switch -q -c parent && mkdir -p src && seq 4 > src/a.txt && git add src/a.txt && git commit -q -m "feat: parent a" && git push -q origin parent && git switch -q main'
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" --base origin/parent 2>&1)
assert_eq 0 $?
assert_contains "$out" "BASE_MOVED: origin/parent"
assert_contains "$out" "COMMON: src/a.txt"

it "base-moved: 上流が別のリモートのブランチでも origin を fetch する"
D=$(fixture upstream_other)
commit_file "$D" src/a.txt "feat: a"
git init -q --bare "$D.up.git"
git -C "$D" remote add up "$D.up.git"
git -C "$D" push -q up feat/work main
git -C "$D" branch -q --set-upstream-to=up/main feat/work
in_other "$D" bash -c 'mkdir -p src && seq 3 > src/a.txt && git add src/a.txt && git commit -q -m "feat: other a"'
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh")
assert_contains "$out" "COMMON: src/a.txt"

it "base-moved: 分岐点が見つからないときは WARN を出して exit 0（通知だけで失敗させない）"
D=$(fixture no_merge_base)
commit_file "$D" src/a.txt "feat: a"
git -C "$D" switch -q --orphan feat/orphan
git -C "$D" commit -q --allow-empty -m "orphan"
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" feat/orphan 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "比較は未確認"

it "base-moved: マージコミットで入った PR は 1 件と数える（--first-parent）"
D=$(fixture merge_count)
commit_file "$D" src/a.txt "feat: a"
in_other "$D" bash -c 'mkdir -p src && git switch -q -c side && seq 2 > src/a.txt && git add src && git commit -q -m "side 1" && seq 3 > src/a.txt && git commit -q -am "side 2" && git switch -q main && git merge -q --no-ff -m "Merge: side" side'
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh")
assert_contains "$out" "1 commits"
assert_contains "$out" "COMMON: src/a.txt"

it "base-moved: 非 ASCII のパスは引用符も八進表記もなしで出す"
D=$(fixture non_ascii)
commit_file "$D" "docs/日本語.md" "feat: doc"
in_other "$D" bash -c 'mkdir -p docs && seq 2 > "docs/日本語.md" && git add docs && git commit -q -m "feat: other doc"'
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh")
assert_contains "$out" "COMMON: docs/日本語.md"

it "base-moved: base 側の移動（rename）は旧パスも共通として数える（--no-renames）"
D=$(fixture rename)
git -C "$D" switch -q main
commit_file "$D" src/old.txt "feat: old" 20
git -C "$D" push -q origin main
git -C "$D" switch -q feat/work
git -C "$D" rebase -q main
commit_file "$D" src/old.txt "feat: edit old" 21
in_other "$D" bash -c 'git mv src/old.txt src/new.txt && git commit -q -m "refactor: move"'
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh")
assert_contains "$out" "COMMON: src/old.txt"

# fail_git <サブコマンド> <終了コード>: そのサブコマンドだけ失敗する git を PATH の先頭に置く
REAL_GIT=$(command -v git)
fail_git() {
  mkdir -p "$TEST_ROOT/fakebin"
  cat >"$TEST_ROOT/fakebin/git" <<SH
#!/usr/bin/env bash
for a in "\$@"; do
  if [[ "\$a" == "$1" ]]; then echo "fatal: injected" >&2; exit $2; fi
done
exec "$REAL_GIT" "\$@"
SH
  chmod +x "$TEST_ROOT/fakebin/git"
  PATH="$TEST_ROOT/fakebin:$PATH"
}
restore_git() {
  rm -f "$TEST_ROOT/fakebin/git"
  PATH="${PATH#"$TEST_ROOT/fakebin:"}"
}

it "base-moved: origin が無い GitHub clone では何も出さずに終わる（誤った WARN を出し続けない）"
D="$TEST_ROOT/bm/no_origin"
make_repo "$D"
git -C "$D" remote add github "https://github.com/example/no_origin.git"
git -C "$D" switch -q -c feat/work
commit_file "$D" src/a.txt "feat: a"
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" 2>&1)
assert_eq 0 $?
assert_eq "" "$out"

it "base-moved: origin が無い GitHub clone でも、--base を渡せば手元のブランチで比べる"
git -C "$D" switch -q main
commit_file "$D" src/a.txt "feat: a on main"
git -C "$D" switch -q feat/work
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" --base main 2>&1)
assert_eq 0 $?
assert_contains "$out" "COMMON: src/a.txt"

it "base-moved: 差分の取得に失敗したら、空の出力（進みなし）と区別できる WARN を出して exit 0"
D=$(fixture diff_fails)
commit_file "$D" src/a.txt "feat: a"
in_other "$D" bash -c 'mkdir -p src && seq 3 > src/a.txt && git add src && git commit -q -m "feat: other a"'
fail_git diff 2
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" 2>&1)
rc=$?
restore_git
assert_eq 0 "$rc"
assert_contains "$out" "WARN:"
assert_contains "$out" "比較は未確認"

it "base-moved: merge-base が 1 以外で失敗したら、終了コードと stderr を出す（共通の祖先なしとは区別する）"
D=$(fixture merge_base_fails)
commit_file "$D" src/a.txt "feat: a"
fail_git merge-base 128
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" 2>&1)
rc=$?
restore_git
assert_eq 0 "$rc"
assert_contains "$out" "終了コード 128"
assert_contains "$out" "fatal: injected"
assert_not_contains "$out" "分岐点が見つかりません"

# warn_git <サブコマンド>: そのサブコマンドが成功しつつ stderr に警告を出す git を PATH の先頭に置く
warn_git() {
  mkdir -p "$TEST_ROOT/fakebin"
  cat >"$TEST_ROOT/fakebin/git" <<SH
#!/usr/bin/env bash
for a in "\$@"; do
  if [[ "\$a" == "$1" ]]; then echo "warning: injected" >&2; break; fi
done
exec "$REAL_GIT" "\$@"
SH
  chmod +x "$TEST_ROOT/fakebin/git"
  PATH="$TEST_ROOT/fakebin:$PATH"
}

it "base-moved: merge-base が成功しつつ stderr に警告を出しても、分岐点に混ぜず通常どおり動く"
D=$(fixture merge_base_warns)
commit_file "$D" src/a.txt "feat: a"
in_other "$D" bash -c 'mkdir -p src && seq 3 > src/a.txt && git add src && git commit -q -m "feat: other a"'
warn_git merge-base
out=$(cd "$D" && "$SCRIPTS_DIR/base-moved.sh" 2>/dev/null)
rc=$?
restore_git
assert_eq 0 "$rc"
assert_contains "$out" "COMMON: src/a.txt"

finish
