#!/usr/bin/env bash
# PreToolUse hook（modules/home/parts/claude-hooks、Rust 製 1 バイナリ）のルールごとのテスト
#
# 実行: bash .claude/tests/hooks.sh
#   claude-hooks を cargo build してから各ルールを --rule で個別に評価する
# 対象: pre-pr-create-check / warn-large-commit / guard-git-push / pre-merge-check /
#       block-secret-commit / block-no-verify / guard-git-add / guard-gh-run-rerun / guard-gh-api /
#       block-main-commit / require-background-wait / guard-branch-base / pre-git-merge-check /
#       guard-git-merge / guard-sbatch / guard-git-stash / guard-git-reset-hard / guard-exclusive-slurm

# hook はリポジトリの Rust クレート（claude-hooks）をビルドした 1 バイナリ。
# lib.sh が HOME を差し替える前にビルドする（~/.cargo/config.toml の sccache 等を使うため）。
# CLAUDE_HOOKS_BIN（絶対パス）で差し替えられる（nix でビルドしたバイナリの検証用）
if [[ -z "${CLAUDE_HOOKS_BIN:-}" ]]; then
  CRATE_DIR=$(cd "$(dirname "$0")/../../modules/home/parts/claude-hooks" && pwd)
  cargo build -q --manifest-path "$CRATE_DIR/Cargo.toml" || exit 1
  CLAUDE_HOOKS_BIN="$CRATE_DIR/target/debug/claude-hooks"
fi
# 渡されたパスが壊れていると（nix build の失敗の文言など）、すべてのテストが失敗として数えられ、
# ビルドの失敗が紛れる。テストは一時のリポジトリへ cd してから呼ぶので、相対のパスは解けない
if [[ "$CLAUDE_HOOKS_BIN" != /* ]]; then
  echo "CLAUDE_HOOKS_BIN は絶対パスで渡してください: $CLAUDE_HOOKS_BIN" >&2
  exit 1
fi
# 引数なしで呼ぶと使い方（rules: …）を出し、終了コード 2 で終わる。-x だけではディレクトリや
# 起動できないバイナリを通す
usage=$("$CLAUDE_HOOKS_BIN" 2>&1 </dev/null) || true
if ! grep -q '^rules: ' <<<"$usage"; then
  echo "claude-hooks を実行できません: $CLAUDE_HOOKS_BIN" >&2
  echo "$usage" >&2
  exit 1
fi

source "$(dirname "$0")/lib.sh"
# pre-pr-create-check は GH_REPO を PR の対象のリポジトリとして読むので、実行する人の環境に左右させない
unset GH_REPO

# ===========================================================================
# pre-pr-create-check
# ===========================================================================

REPO="$TEST_ROOT/prcreate"
make_repo "$REPO"
make_remote "$REPO" github
git -C "$REPO" switch -q -c feat/x
commit_file "$REPO" "a.txt" "feat: a"
git -C "$REPO" push -q -u origin feat/x
cd "$REPO" || exit 1

GOOD_BODY='## Summary
x

## Changes
- y

## Tests
- [ ] z'
GOOD_CMD="gh pr create --title \"feat: x\" --body \"\$(cat <<'EOF'
$GOOD_BODY
EOF
)\""

it "pre-pr-create: 条件を満たす gh pr create は許可する"
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: gh pr create 以外は対象外"
out=$(run_hook pre-pr-create-check "gh pr view 12")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: --web は deny"
out=$(run_hook pre-pr-create-check "$GOOD_CMD --web")
assert_eq deny "$(decision "$out")"
# パスで書いた gh も gh として見る
out=$(run_hook pre-pr-create-check "/usr/bin/$GOOD_CMD --web")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--web"

it "pre-pr-create: --title がなければ deny"
out=$(run_hook pre-pr-create-check "gh pr create --body \"$GOOD_BODY\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--title"

it "pre-pr-create: --title が 70 文字を超えたら deny（文字数は UTF-8 の文字単位）"
long_title=$(printf 'あ%.0s' $(seq 71))
out=$(run_hook pre-pr-create-check "gh pr create --title \"$long_title\" --body \"$GOOD_BODY\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "70"
ok_title=$(printf 'あ%.0s' $(seq 70))
out=$(run_hook pre-pr-create-check "gh pr create --title \"$ok_title\" --body \"$GOOD_BODY\"")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: -t の短縮形も title として認識する"
out=$(run_hook pre-pr-create-check "gh pr create -t \"feat: x\" -b \"$GOOD_BODY\"")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: --body / --body-file がなければ deny（--fill は本文とみなさない）"
out=$(run_hook pre-pr-create-check "gh pr create --title \"feat: x\" --fill")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--body"

it "pre-pr-create: 本文に見出しが欠けていたら欠けた見出しを列挙して deny（見出しは常に英語）"
out=$(run_hook pre-pr-create-check "gh pr create --title \"feat: x\" --body \"## Summary
x\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## Changes"
assert_contains "$(reason "$out")" "## Tests"
assert_not_contains "$(reason "$out")" "## Summary"

it "pre-pr-create: 日本語の見出し（## 概要 / ## 変更点 / ## テスト）は受け付けない"
out=$(run_hook pre-pr-create-check "gh pr create --title \"feat: x\" --body \"## 概要
x

## 変更点
- y

## テスト
- z\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## Summary"

it "pre-pr-create: --body-file の内容で見出しを判定する"
printf '%s\n' "$GOOD_BODY" > "$TEST_ROOT/body.md"
out=$(run_hook pre-pr-create-check "gh pr create --title \"feat: x\" --body-file $TEST_ROOT/body.md")
assert_eq allow "$(decision "$out")"
echo "## Summary" > "$TEST_ROOT/bad-body.md"
out=$(run_hook pre-pr-create-check "gh pr create --title \"feat: x\" -F $TEST_ROOT/bad-body.md")
assert_eq deny "$(decision "$out")"

it "pre-pr-create: 読めない --body-file は、先に別の呼び出しで書き、絶対パスで渡すよう示す"
for cmd in "gh pr create --title \"feat: x\" --body-file $TEST_ROOT/no-such-body.md" \
  "S=$TEST_ROOT; gh pr create --title \"feat: x\" --body-file \$S/body.md"; do
  out=$(run_hook pre-pr-create-check "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "別の呼び出し"
  assert_contains "$(reason "$out")" "絶対パス"
done

it "pre-pr-create: 未プッシュコミットがあれば deny"
commit_file "$REPO" "b.txt" "feat: b"
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "未プッシュ"
assert_contains "$(reason "$out")" "別の呼び出し"
git -C "$REPO" push -q

it "pre-pr-create: 上流ブランチがなければ deny"
git -C "$REPO" switch -q -c feat/no-upstream
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git push"
assert_contains "$(reason "$out")" "別の呼び出し"
out=$(run_hook pre-pr-create-check "git push -u origin HEAD && $GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "別の呼び出し"
git -C "$REPO" switch -q feat/x

it "pre-pr-create: --head 指定時は現在ブランチの push 状態を見ない"
git -C "$REPO" switch -q feat/no-upstream
out=$(run_hook pre-pr-create-check "$GOOD_CMD --head feat/x")
assert_eq allow "$(decision "$out")"
git -C "$REPO" switch -q feat/x

it "pre-pr-create: cd 先のリポジトリで判定する"
OTHER="$TEST_ROOT/prcreate-other"
make_repo "$OTHER"
git -C "$OTHER" switch -q -c feat/y
cd "$TEST_ROOT" || exit 1
out=$(run_hook pre-pr-create-check "cd $OTHER && $GOOD_CMD")
assert_eq deny "$(decision "$out")"
cd "$REPO" || exit 1

it "pre-pr-create: ssh と sudo -i の中の gh は、cd 先でなく、その中で走るので手元で検査できないと示して止める"
# 手元の場所の検査（本文の見出し、上流ブランチ）は、接続先の PR に当たらないので理由に混ぜない
out=$(run_hook pre-pr-create-check "ssh host gh pr create --title 'feat: x' --body y")
assert_eq deny "$(decision "$out")"
assert_eq "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます" "$(reason "$out")"
out=$(run_hook pre-pr-create-check "ssh host $GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます"
assert_not_contains "$(reason "$out")" "cd 先"
out=$(run_hook pre-pr-create-check "sudo -i $GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます"
assert_not_contains "$(reason "$out")" "cd 先"
# 展開しない cd の行き先は、移れない cd 先として示す
out=$(run_hook pre-pr-create-check "cd \"\$NOWHERE\" && $GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "コマンド中の cd 先に移動できません"

# --- トピックが 1 つか（新しい ADR の数）: マージの時点と同じ判定を PR を作る時点で当てる ---
# base から head までに加わった docs/adr 直下の NNNN-*.md を数え、2 件以上なら止める
git -C "$REPO" switch -q -c feat/two-adrs main
commit_file "$REPO" "docs/adr/0002-use-b.md" "docs(adr): b"
commit_file "$REPO" "docs/adr/0003-use-c.md" "docs(adr): c"
git -C "$REPO" push -q -u origin feat/two-adrs
git -C "$REPO" switch -q -c feat/stacked
commit_file "$REPO" "docs/adr/0004-use-d.md" "docs(adr): d"
git -C "$REPO" push -q -u origin feat/stacked
git -C "$REPO" switch -q -c feat/one-adr main
commit_file "$REPO" "docs/adr/0005-use-e.md" "docs(adr): e"
commit_file "$REPO" "docs/adr/README.md" "docs(adr): readme"
git -C "$REPO" push -q -u origin feat/one-adr

it "pre-pr-create: 新しい ADR を 2 件以上加えるブランチは deny し、ADR を挙げて /topic-triage を示す"
git -C "$REPO" switch -q feat/two-adrs
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"
assert_contains "$(reason "$out")" "docs/adr/0003-use-c.md"
assert_contains "$(reason "$out")" "/topic-triage"
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC=<理由>"
# 「〜の場合は別」のような条件を文言に列挙しない（ADR-0023）
assert_not_contains "$(reason "$out")" "まとめて加える"

it "pre-pr-create: リポジトリのサブディレクトリから実行しても docs/adr を数える"
out=$(run_hook pre-pr-create-check "cd '$REPO/docs/adr' && $GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"
# diff.relative の設定に左右されない
git -C "$REPO" config diff.relative true
out=$(run_hook pre-pr-create-check "cd '$REPO/docs/adr' && $GOOD_CMD")
git -C "$REPO" config --unset diff.relative
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"

it "pre-pr-create: 新しい ADR が 1 件以下（README は数えない）なら許可する"
git -C "$REPO" switch -q feat/one-adr
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: --base で指定したブランチから数える"
git -C "$REPO" switch -q feat/stacked
out=$(run_hook pre-pr-create-check "$GOOD_CMD --base feat/two-adrs")
assert_eq allow "$(decision "$out")"
out=$(run_hook pre-pr-create-check "$GOOD_CMD -B feat/two-adrs")
assert_eq allow "$(decision "$out")"
# --base=x の形も読む（読めないと数えられずに通るので、数えて止まる組で確かめる）
git -C "$REPO" switch -q feat/two-adrs
out=$(run_hook pre-pr-create-check "$GOOD_CMD --base=main")
assert_eq deny "$(decision "$out")"
git -C "$REPO" switch -q feat/stacked
# --base が無ければ既定ブランチから数える（積んだ下のブランチの ADR も入る）
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0004-use-d.md"
# --base が無ければ、gh と同じく branch.<head>.gh-merge-base を先に使う
git -C "$REPO" config branch.feat/stacked.gh-merge-base feat/two-adrs
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
git -C "$REPO" config --unset branch.feat/stacked.gh-merge-base
assert_eq allow "$(decision "$out")"

it "pre-pr-create: --head で指定したブランチで数える"
git -C "$REPO" switch -q feat/one-adr
out=$(run_hook pre-pr-create-check "$GOOD_CMD --head feat/two-adrs")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"
out=$(run_hook pre-pr-create-check "$GOOD_CMD --head=feat/two-adrs")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"

it "pre-pr-create: -R / --repo / GH_REPO で origin と別のリポジトリに出す PR は数えない"
git -C "$REPO" switch -q feat/two-adrs
for cmd in "$GOOD_CMD -R other/proj" "$GOOD_CMD --repo=other/proj" "GH_REPO=other/proj $GOOD_CMD"; do
  out=$(run_hook pre-pr-create-check "$cmd")
  assert_eq allow "$(decision "$out")"
done
# GH_REPO は gh pr create に渡るものだけを見る（環境の値、別のコマンドの前置き、env -u）
out=$(GH_REPO=other/proj run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq allow "$(decision "$out")"
out=$(run_hook pre-pr-create-check "GH_REPO=other/proj gh repo view && $GOOD_CMD")
assert_eq deny "$(decision "$out")"
out=$(GH_REPO=other/proj run_hook pre-pr-create-check "env -u GH_REPO $GOOD_CMD")
assert_eq deny "$(decision "$out")"
# origin と同じリポジトリなら数える（HOST/ の有無、URL の形、大文字と小文字を問わない）
origin_url=$(git -C "$REPO" remote get-url origin)
git -C "$REPO" remote set-url origin git@github.com:example/prcreate.git
for repo in example/prcreate github.com/Example/prcreate https://github.com/example/prcreate; do
  out=$(run_hook pre-pr-create-check "$GOOD_CMD -R $repo")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"
done
git -C "$REPO" remote set-url origin "$origin_url"

it "pre-pr-create: 比べる base が見つからなければ数えずに通す（マージの時点の検査が GitHub の変更ファイルで数える）"
git -C "$REPO" switch -q feat/two-adrs
out=$(run_hook pre-pr-create-check "$GOOD_CMD --base no-such-branch")
assert_eq allow "$(decision "$out")"
# 既定ブランチが main / master でなく origin/HEAD も無い clone（ADR の無い他のプロジェクトに多い）
TRUNK="$TEST_ROOT/prcreate-trunk"
git init -q -b trunk "$TRUNK"
commit_file "$TRUNK" "a.txt" "feat: a"
git -C "$TRUNK" init -q --bare "$TRUNK.git"
git -C "$TRUNK" remote add origin "$TRUNK.git"
git -C "$TRUNK" push -q -u origin trunk
out=$(run_hook pre-pr-create-check "cd '$TRUNK' && $GOOD_CMD")
assert_eq allow "$(decision "$out")"
git -C "$REPO" switch -q feat/two-adrs

it "pre-pr-create: cd 先に移れなければ、手元の cwd のブランチの ADR は数えない"
out=$(run_hook pre-pr-create-check "cd '$TEST_ROOT/no-such-dir' && $GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "コマンド中の cd 先に移動できません"
assert_not_contains "$(reason "$out")" "/topic-triage"

it "pre-pr-create: ALLOW_MULTI_TOPIC=<理由> なら新しい ADR が複数でも PR を作れる"
out=$(run_hook pre-pr-create-check "ALLOW_MULTI_TOPIC=テスト用 $GOOD_CMD")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: ALLOW_MULTI_TOPIC=1 は、本来の文言に理由を求める行を足して止める"
out=$(run_hook pre-pr-create-check "ALLOW_MULTI_TOPIC=1 $GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "/topic-triage"
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC には理由を記載"
# エスケープは新しい ADR の検査だけを外す（ほかの検査は残る）
out=$(run_hook pre-pr-create-check "ALLOW_MULTI_TOPIC=テスト用 $GOOD_CMD --web")
assert_eq deny "$(decision "$out")"
assert_not_contains "$(reason "$out")" "/topic-triage"
git -C "$REPO" switch -q feat/x

# --- fork（upstream リモートのある clone）: こちらの規約は検査しない ---
FORK="$TEST_ROOT/prcreate-fork"
make_repo "$FORK"
make_remote "$FORK"
git -C "$FORK" remote add upstream https://github.com/example/theirs.git
git -C "$FORK" switch -q -c fix/theirs
commit_file "$FORK" "b.txt" "Fix the thing upstream style"
git -C "$FORK" push -q -u origin fix/theirs
cd "$FORK" || exit 1
THEIR_BODY='## Problem
x

## Checklist
- [x] scope'

it "pre-pr-create: fork では 70 文字超の件名と上流の template の本文を許可する"
long_title="fix(typescript): surface a tsserver crash on every cross-file query, not only the first"
out=$(run_hook pre-pr-create-check "gh pr create --repo example/theirs --head me:fix/theirs --title \"$long_title\" --body \"$THEIR_BODY\"")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: fork では新しい ADR が 2 件以上でも許可する（上流の流儀が正）"
git -C "$FORK" switch -q -c fix/two-adrs main
commit_file "$FORK" "docs/adr/0002-use-b.md" "Add ADR b"
commit_file "$FORK" "docs/adr/0003-use-c.md" "Add ADR c"
git -C "$FORK" push -q -u origin fix/two-adrs
out=$(run_hook pre-pr-create-check "gh pr create --title \"Add ADRs\" --body \"$THEIR_BODY\"")
assert_eq allow "$(decision "$out")"
git -C "$FORK" switch -q fix/theirs

it "pre-pr-create: fork でも --body / --body-file がなければ deny（見出しは求めない）"
out=$(run_hook pre-pr-create-check "gh pr create --title \"$long_title\" --fill")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--body"
assert_contains "$(reason "$out")" "上流の PR template"

it "pre-pr-create: fork でも --web は deny"
out=$(run_hook pre-pr-create-check "gh pr create --title \"$long_title\" --body \"$THEIR_BODY\" --web")
assert_eq deny "$(decision "$out")"

cd "$REPO" || exit 1

# ===========================================================================
# warn-large-commit
# ===========================================================================

REPO="$TEST_ROOT/large"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1

it "warn-large-commit: 小さな変更では何も出さない"
seq 3 > small.txt && git add small.txt
out=$(run_hook warn-large-commit 'git commit -m "feat: small"')
assert_eq "" "$out"
git commit -q -m "feat: small"

it "warn-large-commit: git commit 以外は対象外"
out=$(run_hook warn-large-commit 'git log --grep commit')
assert_eq "" "$out"

it "warn-large-commit: 5 ファイル以上なら additionalContext で件数を伝える（deny しない）"
for i in 1 2 3 4 5; do echo "$i" > "f$i.txt"; done
git add f1.txt f2.txt f3.txt f4.txt f5.txt
out=$(run_hook warn-large-commit 'git commit -m "feat: many"')
assert_eq allow "$(decision "$out")"
assert_contains "$(additional_context "$out")" "5 ファイル"
git commit -q -m "feat: many"

it "warn-large-commit: 100 行以上なら additionalContext で行数を伝える"
seq 100 > big.txt && git add big.txt
out=$(run_hook warn-large-commit 'git commit -m "feat: big"')
assert_contains "$(additional_context "$out")" "100 行"
git commit -q -m "feat: big"

it "warn-large-commit: -a 指定時は未ステージの変更も数える"
seq 200 > big.txt
out=$(run_hook warn-large-commit 'git commit -am "feat: big2"')
assert_contains "$(additional_context "$out")" "行"
out=$(run_hook warn-large-commit 'git commit -m "feat: nothing staged"')
assert_eq "" "$out"
git checkout -q big.txt

it "warn-large-commit: -C 指定のリポジトリを見る"
cd "$TEST_ROOT" || exit 1
seq 150 > "$REPO/c.txt" && git -C "$REPO" add c.txt
out=$(run_hook warn-large-commit "git -C $REPO commit -m 'feat: c'")
assert_contains "$(additional_context "$out")" "150 行"
git -C "$REPO" commit -q -m "feat: c"

# ===========================================================================
# guard-git-push: feature branch への --force-with-lease は open PR があっても許可
# （origin/main にリベースしてからマージコミットする運用。--force / +refspec は引き続き禁止）
# ===========================================================================

REPO="$TEST_ROOT/push"
make_repo "$REPO"
make_remote "$REPO" github
git -C "$REPO" switch -q -c feat/x
commit_file "$REPO" "a.txt" "feat: a"
git -C "$REPO" push -q -u origin feat/x
cd "$REPO" || exit 1

it "guard-git-push: 既存動作 — main への push は deny、feature への通常 push は許可"
out=$(run_hook guard-git-push 'git push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push 'git push')
assert_eq allow "$(decision "$out")"

it "guard-git-push: open PR のあるブランチへの --force-with-lease も許可し、gh に問い合わせない"
make_fake_gh '"pr view feat/x"*) echo OPEN ;;'
out=$(run_hook guard-git-push 'git push --force-with-lease')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-push 'git push --force-with-lease origin feat/x')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-git-push: --force-with-lease でも main 宛ては deny"
out=$(run_hook guard-git-push 'git push --force-with-lease origin main')
assert_eq deny "$(decision "$out")"

it "guard-git-push: --force / +refspec は feature branch でも deny"
out=$(run_hook guard-git-push 'git push --force')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--force-with-lease"
out=$(run_hook guard-git-push 'git push origin +feat/x')
assert_eq deny "$(decision "$out")"

it "guard-git-push: ALLOW_PROTECTED_PUSH=<理由> で迂回できる"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH=テスト用 git push --force')
assert_eq allow "$(decision "$out")"

it "guard-git-push: bash -c の文字列の中と、sudo などで包んだ git push も見る。エスケープは文字列の中でも読む"
for cmd in "bash -c 'git push --force'" 'sudo -E git push origin main' 'eval git push --force' \
  "sh -c 'cd . && git push origin main'" 'xargs git push origin main'; do
  out=$(run_hook guard-git-push "$cmd")
  assert_eq deny "$(decision "$out")"
done
out=$(run_hook guard-git-push "bash -c 'ALLOW_PROTECTED_PUSH=テスト用 git push --force'")
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-push "bash -c 'ALLOW_PROTECTED_PUSH=1 git push --force'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"

it "guard-git-push: bash / sh のまとめたオプションの中の -o / -O の値は次の語で、-c の文字列の中も見る"
for cmd in "bash -oc pipefail 'git push --force'" "bash -Oc extglob 'git push --force'" "sh -oec pipefail 'git push --force'"; do
  out=$(run_hook guard-git-push "$cmd")
  assert_eq deny "$(decision "$out")"
done

# エスケープの値は理由の文字列（ADR-0023）。`1` や 1 語の英単語は弾き、短い値を拒む文言はしきい値を明かさない
it "escape: 値が 1 や短い語なら、理由を求めて止める（しきい値は明かさない）"
for value in 1 true force allow 確認; do
  out=$(run_hook guard-git-push "ALLOW_PROTECTED_PUSH=$value git push --force")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"
  # 本来の文言（何を止めたか、正規の修正方法）は残す
  assert_contains "$(reason "$out")" "force push は禁止"
  assert_not_contains "$(reason "$out")" "6"
  assert_not_contains "$(reason "$out")" "文字"
done

it "escape: 展開（\$VAR、\$(…)）は記録に残らないので幅に数えず、字面だけで 6 に満たなければ理由を求める"
for value in '$REASON' '"$(cat r.txt)"' '"ab $X"' 'ab$X' '<(true)' '$((1+1))'; do
  out=$(run_hook guard-git-push "ALLOW_PROTECTED_PUSH=$value git push --force")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"
done

it "escape: 展開を含んでも、字面の部分だけで 6 以上あれば通す（PR 番号などを展開で足せる）"
for value in '"rebase のみ $X"' '"rebase のみ、PR #$(gh pr view --json number -q .number)"' 'abcdef$X'; do
  out=$(run_hook guard-git-push "ALLOW_PROTECTED_PUSH=$value git push --force")
  assert_eq allow "$(decision "$out")"
done

it "escape: ANSI-C 文字列やバックスラッシュで幅を水増しできない"
for value in "\$'\\t\\t\\t'" 'a\ \ \ \ b' "\$'abc'"; do
  out=$(run_hook guard-git-push "ALLOW_PROTECTED_PUSH=$value git push --force")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"
done

it "escape: 全角空白や NBSP でも幅を水増しできず、それだけの値は付けていない扱い"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH="a　　　b" git push --force')
assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH="a    b" git push --force')
assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH="　　　" git push --force')
assert_contains "$(reason "$out")" "force push は禁止"
assert_not_contains "$(reason "$out")" "理由を記載"

it "escape: 空白を除いた幅（ASCII は 1、それ以外は 2）が 6 以上の理由なら通す"
for value in aaaaaa 差分同一 '"  abcdef  "' '"rebase のみ"'; do
  out=$(run_hook guard-git-push "ALLOW_PROTECTED_PUSH=$value git push --force")
  assert_eq allow "$(decision "$out")"
done

it "escape: 理由を求めるのは、そのルールが止める命令のときだけ"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH=1 git push origin feat/x')
assert_eq allow "$(decision "$out")"
out=$(run_hook block-no-verify 'ALLOW_NO_VERIFY=1 git status')
assert_eq allow "$(decision "$out")"

it "escape: 同じ名前の代入が複数あれば、短い値が 1 つでもあれば理由を求める"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH=1 git push --force; ALLOW_PROTECTED_PUSH=placeholder true')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"

it "escape: 0 と空（空白だけを含む）の値はエスケープを付けていないものとして扱う"
for value in 0 '' '"     "'; do
  out=$(run_hook guard-git-push "ALLOW_PROTECTED_PUSH=$value git push --force")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "force push は禁止"
  assert_not_contains "$(reason "$out")" "理由を記載"
done

it "escape: 幅は空白を全部除いて数える（内側の空白で水増しできない）"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH="a    b" git push --force')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH には理由を記載"

it "escape: 止める文言は、理由を値にした付け方と、機械で判定できる理由なら issue に書くことを案内する"
out=$(run_hook guard-git-push 'git push --force')
assert_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH=<理由>"
assert_contains "$(reason "$out")" "issue"
assert_not_contains "$(reason "$out")" "ALLOW_PROTECTED_PUSH=1"

it "guard-git-push: env などで包んだ git push も見て、値を取るオプションの値を -f と取り違えない"
for cmd in 'env FOO=1 git push --force' 'command git push origin main' 'nohup git push --force'; do
  out=$(run_hook guard-git-push "$cmd")
  assert_eq deny "$(decision "$out")"
done
out=$(run_hook guard-git-push 'git push -ofoo origin feat/x')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-push 'git push -o --force origin feat/x')
assert_eq allow "$(decision "$out")"

it "guard-git-push: GitHub リモートがなければ gh を呼ばず許可"
LOCAL="$TEST_ROOT/push-local"
make_repo "$LOCAL"
make_remote "$LOCAL"
git -C "$LOCAL" switch -q -c feat/x
cd "$LOCAL" || exit 1
make_fake_gh ''
out=$(run_hook guard-git-push 'git push origin main')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

# ===========================================================================
# block-main-commit: flake.lock だけの更新は main でも通す
# ===========================================================================
# nix-rebuild update は検証済みの flake.lock を main に直接コミットする正規フロー。
# git 側の pre-commit hook（modules/home/parts/git.nix）は既にこれを例外として
# 許可しているので、PreToolUse hook 側も同じ判定にする（片方だけ塞ぐと update が止まる）

REPO="$TEST_ROOT/lock"
make_repo "$REPO"
make_remote "$REPO" github
cd "$REPO" || exit 1
echo '{"nodes":{}}' > flake.lock
echo 'x' > other.txt
git add flake.lock other.txt
git commit -q -m "chore: add lock"

it "block-main-commit: main でも flake.lock だけの変更なら通す"
echo '{"nodes":{"updated":1}}' > flake.lock
git add flake.lock
out=$(run_hook block-main-commit 'git commit -m "chore(flake): update (r995)"')
assert_eq allow "$(decision "$out")"

it "block-main-commit: flake.lock 以外が混ざっていれば main では deny"
echo 'y' > other.txt
git add other.txt
out=$(run_hook block-main-commit 'git commit -m "chore: mixed"')
assert_eq deny "$(decision "$out")"
for cmd in 'env FOO=1 git commit -m "chore: mixed"' 'command git commit -m "chore: mixed"'; do
  out=$(run_hook block-main-commit "$cmd")
  assert_eq deny "$(decision "$out")"
done
git restore --staged other.txt && git checkout -q other.txt

it "block-main-commit: flake.lock の削除・リネームは main では deny"
git rm -q --cached flake.lock
out=$(run_hook block-main-commit 'git commit -m "chore(flake): remove"')
assert_eq deny "$(decision "$out")"
git restore --staged flake.lock

it "block-main-commit: 何もステージされていなければ main では deny"
git restore --staged flake.lock 2>/dev/null || true
git checkout -q flake.lock
out=$(run_hook block-main-commit 'git commit -m "chore: nothing"')
assert_eq deny "$(decision "$out")"

it "block-main-commit: feature ブランチでは従来どおり何でも通す"
git switch -q -c feat/x
echo 'z' > other.txt && git add other.txt
out=$(run_hook block-main-commit 'git commit -m "feat: x"')
assert_eq allow "$(decision "$out")"
git switch -q main

# ===========================================================================
# GitHub リモートがなければ PR フロー適用外
# ===========================================================================
# PR を作れないリポジトリで feature branch を強制しても、マージする手段がなく
# 作業が進まない。判定は「GitHub リモートがあるか」。
# GitHub 以外のリモートは扱わない前提なので、GitHub でなければローカル専用と同じ扱い

LOCAL="$TEST_ROOT/localonly"
make_repo "$LOCAL"
cd "$LOCAL" || exit 1

it "block-main-commit: リモートのないリポジトリなら main でもコミットできる"
echo 'x' > a.txt && git add a.txt
out=$(run_hook block-main-commit 'git commit -m "feat: x"')
assert_eq allow "$(decision "$out")"

it "block-main-commit: GitHub 以外のリモートだけならローカル専用と同じ扱い"
make_remote "$LOCAL"
out=$(run_hook block-main-commit 'git commit -m "feat: x"')
assert_eq allow "$(decision "$out")"

it "guard-git-push: GitHub リモートがなければ main への push も force push も止めない"
# 直前の make_remote でローカルの bare リモートだけがある状態
BARE="$TEST_ROOT/localonly.git"
out=$(run_hook guard-git-push "git push origin main")
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-push "git push --force origin main")
assert_eq allow "$(decision "$out")"
assert_file_exists "$BARE"

it "block-main-commit / guard-git-push: GitHub リモートがあれば従来どおり止める"
git remote add gh https://github.com/example/localonly.git
out=$(run_hook block-main-commit 'git commit -m "feat: x"')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push "git push origin main")
assert_eq deny "$(decision "$out")"
git remote remove gh

it "block-main-commit: -C で指定した別リポジトリのリモートで判定する"
cd "$TEST_ROOT" || exit 1
out=$(run_hook block-main-commit "git -C $LOCAL commit -m 'feat: x'")
assert_eq allow "$(decision "$out")"
out=$(run_hook block-main-commit "git -C $TEST_ROOT/lock commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"
# 1 回の env の中では最後の -C だけが効く（前の -C からの相対ではない）
out=$(run_hook block-main-commit "env -C $TEST_ROOT/lock -C $LOCAL git commit -m 'feat: x'")
assert_eq allow "$(decision "$out")"
for cmd in "env -C $TEST_ROOT/lock git commit -m 'feat: x'" "env --chdir=$TEST_ROOT/lock git commit -m 'feat: x'" \
  "env -C $LOCAL -C lock git commit -m 'feat: x'" "env -C $LOCAL env -C $TEST_ROOT/lock git commit -m 'feat: x'"; do
  out=$(run_hook block-main-commit "$cmd")
  assert_eq deny "$(decision "$out")"
done
# sudo -D / --chdir もディレクトリを変える
out=$(run_hook block-main-commit "sudo -E -D $LOCAL git commit -m 'feat: x'")
assert_eq allow "$(decision "$out")"
out=$(run_hook block-main-commit "sudo -E --chdir=$TEST_ROOT/lock git commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"
cd "$REPO" || exit 1

it "block-main-commit: コマンド置換やサブシェルの中の cd は親の対象ディレクトリを変えない"
out=$(run_hook block-main-commit "echo \$(cd $LOCAL && pwd) && git commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"
out=$(run_hook block-main-commit "(cd $LOCAL); git commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"

it "block-main-commit: 前の cd はサブシェルの中のコマンドにも効く"
out=$(run_hook block-main-commit "cd $LOCAL; (git commit -m 'feat: x')")
assert_eq allow "$(decision "$out")"
out=$(run_hook block-main-commit "cd $LOCAL && echo \$(git commit -m 'feat: x')")
assert_eq allow "$(decision "$out")"

it "block-main-commit: sudo -i と ssh の中のコマンドは手元の cwd で走らないので、手元のブランチで判定しない"
for cmd in "sudo -i git commit -m 'feat: x'" "sudo -i bash -c 'cd ~ && git commit -m x'" \
  "ssh host git commit -m 'feat: x'" "ssh host 'cd $REPO && git commit -m x'" "ssh host git -C $REPO commit -m x"; do
  out=$(run_hook block-main-commit "$cmd")
  assert_eq allow "$(decision "$out")"
done
# sudo -i は手元で走るので、中の絶対パスは手元のリポジトリを指す
for cmd in "ssh host true; git commit -m 'feat: x'" "sudo -i bash -c 'cd $REPO && git commit -m x'" \
  "sudo -i git -C $REPO commit -m x"; do
  out=$(run_hook block-main-commit "$cmd")
  assert_eq deny "$(decision "$out")"
done

it "block-main-commit: bash -c の文字列の中の cd は、同じ文字列の中の後ろのコマンドにだけ効く。前の cd は文字列の中にも効く"
out=$(run_hook block-main-commit "bash -c 'cd $LOCAL && git commit -m \"feat: x\"'")
assert_eq allow "$(decision "$out")"
out=$(run_hook block-main-commit "cd $LOCAL; bash -c 'git commit -m \"feat: x\"'")
assert_eq allow "$(decision "$out")"
out=$(run_hook block-main-commit "bash -c 'git commit -m \"feat: x\"'")
assert_eq deny "$(decision "$out")"
out=$(run_hook block-main-commit "bash -c 'cd $LOCAL'; git commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"
out=$(run_hook block-main-commit "env -C $LOCAL bash -c 'git commit -m \"feat: x\"'")
assert_eq allow "$(decision "$out")"

it "block-main-commit: env などで包んだ cd は対象ディレクトリを変えず、builtin / command の cd は変える"
# env は外部コマンドの cd を探して失敗するので、後ろの git は元のディレクトリ（$REPO、main）で走る
for cmd in "env cd $LOCAL; git commit -m 'feat: x'" "nohup cd $LOCAL; git commit -m 'feat: x'"; do
  out=$(run_hook block-main-commit "$cmd")
  assert_eq deny "$(decision "$out")"
done
for cmd in "/usr/bin/time cd $LOCAL; git commit -m 'feat: x'" "command time cd $LOCAL; git commit -m 'feat: x'"; do
  out=$(run_hook block-main-commit "$cmd")
  assert_eq deny "$(decision "$out")"
done
# 修飾しない time は bash の予約語で、cd は同じシェルで走る
for cmd in "builtin cd $LOCAL && git commit -m 'feat: x'" "command cd $LOCAL && git commit -m 'feat: x'" \
  "time cd $LOCAL; git commit -m 'feat: x'" "time -p cd $LOCAL; git commit -m 'feat: x'"; do
  out=$(run_hook block-main-commit "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "block-main-commit: コマンドより後ろの cd は対象ディレクトリにしない"
# $REPO（github リモートあり・main）で実行。後ろの cd $LOCAL（リモートなし）を見てしまうと allow になる
out=$(run_hook block-main-commit "git commit -m 'feat: x' && cd $LOCAL")
assert_eq deny "$(decision "$out")"

# ===========================================================================
# 全 hook 共通: ヒアドキュメント本文は「実行されるコマンド」ではない
# ===========================================================================
# hook はコマンド文字列を正規表現で検査するため、ヒアドキュメント本文に現れる
# だけの `git push` / `gh pr create` 等に誤反応してはならない。
# ただし本文の中身を読む検査（PR 本文の見出し）は、本文がヒアドキュメントで
# 渡されるのが普通なので、これまでどおり本文を読めなければならない。

REPO="$TEST_ROOT/heredoc"
make_repo "$REPO"
make_remote "$REPO" github
cd "$REPO" || exit 1

# ファイルにコマンド例を書き出すだけのコマンド（実行はしない）
write_doc() {
  printf 'cat > doc.md <<%s\n%s\nSENTINEL\n' "'SENTINEL'" "$1"
}

it "heredoc: 本文中の git push はコマンドとみなさない（guard-git-push）"
out=$(run_hook guard-git-push "$(write_doc '例: git push origin main は禁止')")
assert_eq allow "$(decision "$out")"

it "heredoc: 本文中の git commit はコマンドとみなさない（block-main-commit / warn-large-commit）"
git switch -q main
seq 200 > many.txt && git add many.txt
out=$(run_hook block-main-commit "$(write_doc '例: git commit -m msg')")
assert_eq allow "$(decision "$out")"
out=$(run_hook warn-large-commit "$(write_doc '例: git commit -m msg')")
assert_eq "" "$out"
git reset -q && rm -f many.txt
git switch -q feat/x 2>/dev/null || git switch -q -c feat/x

it "heredoc: 本文中の gh-wait-review.sh はコマンドとみなさない（require-background-wait）"
out=$(run_hook require-background-wait "$(write_doc '待機は gh-wait-review.sh を使う')")
assert_eq allow "$(decision "$out")"

it "heredoc: gh pr merge の本文が gh pr create に触れても PR 作成とみなさない"
MERGE_BODY='## Why
x

## What
- push と gh pr create は別々に実行する

## Impact
なし'
MERGE_CMD="gh pr merge 1 --merge --delete-branch --subject \"Merge: x\" --body \"\$(cat <<'EOF'
$MERGE_BODY
EOF
)\""
out=$(run_hook pre-pr-create-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "heredoc: 本物のコマンドは引き続き検出する（誤って全部素通しにしない）"
out=$(run_hook guard-git-push "$(write_doc '例: 説明')"$'\ngit push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook pre-pr-create-check "$(write_doc '例: 説明')"$'\ngh pr create --fill')
assert_eq deny "$(decision "$out")"

it "heredoc: 算術式のシフト演算子をヒアドキュメントの開始とみなさない"
# $(( 1 << 3 )) の << はシフト演算子。これを開始と誤認すると区切り文字が現れず、
# 以降の行がすべて本文としてマスクされ、実コマンドが hook から見えなくなる
out=$(run_hook guard-git-push 'SIZE=$(( 1 << 3 ))
git push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push 'if (( n << 2 )); then :; fi
git push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push 'if (( n << foo &&
 m )); then :; fi
git push origin main')
assert_eq deny "$(decision "$out")"

it "heredoc: クォートの中の << はヒアドキュメントの開始ではない（bash 版の既知の限界）"
out=$(run_hook guard-git-push 'echo "a << b"; git push origin main')
assert_eq deny "$(decision "$out")"

it "heredoc: ハイフン入りの区切り文字でも本文の後ろの実コマンドは検出する"
out=$(run_hook guard-git-push "cat > doc.md <<END-TEXT
例: git push origin main は禁止
END-TEXT
git push origin main")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push "cat > doc.md <<123
例: git push origin main は禁止
123")
assert_eq allow "$(decision "$out")"

it "heredoc: PR 本文がヒアドキュメントでも見出しを読める（pre-pr-create）"
git -C "$REPO" push -q -u origin feat/x 2>/dev/null || true
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq allow "$(decision "$out")"
out=$(run_hook pre-pr-create-check "gh pr create --title \"feat: x\" --body \"\$(cat <<'EOF'
## Summary
x
EOF
)\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## Changes"

it "heredoc: マージ本文がヒアドキュメントでも見出しを読める（pre-merge-check）"
# make_fake_gh_merge <behind_by> [reviews] [files]: マージ可能な PR #1 の gh 応答。
# compare は base...head の遅れを、reviews は最新の bot レビューが見た commit を返す
# （既定は head と同じ abc = 最後の push がレビュー済み）。files は PR の変更ファイルを
# "status<TAB>path" の行で返す（既定は無し）
make_fake_gh_merge() {
  local reviews="${2:-echo abc}" files="${3:-true}"
  make_fake_gh '"pr view --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo "{\"name\":\"ci\",\"status\":\"completed\",\"conclusion\":\"success\"}" ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/main...abc"*) '"$1"' ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*) '"$reviews"' ;;
  "api --paginate repos/example/heredoc/pulls/1/files"*) '"$files"' ;;
  "api graphql"*) echo "[]" ;;'
}
make_fake_gh_merge 'echo 0'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"
BAD_MERGE_CMD="gh pr merge 1 --merge --delete-branch --subject \"Merge: x\" --body \"\$(cat <<'EOF'
## Why
x
EOF
)\""
out=$(run_hook pre-merge-check "$BAD_MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## What"

it "heredoc: 本文中の見出しでゲートを通せない（見出し検査は実際に渡す本文だけを見る）"
# ヒアドキュメント本文に「見出しの揃った例」を書いておき、実際の PR 本文には
# 見出しを書かない。本文検査が本文の外まで拾うと、これで deny をすり抜けられる
DECOY_CREATE="cat > doc.md <<'SENTINEL'
gh pr create --title t --body \"## Summary x ## Changes y ## Tests z\"
SENTINEL
gh pr create --title \"feat: x\" --body \"見出しなし\""
out=$(run_hook pre-pr-create-check "$DECOY_CREATE")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## Summary"

DECOY_MERGE="cat > doc.md <<'SENTINEL'
gh pr merge 1 --merge --delete-branch --body \"## Why x ## What y ## Impact z\"
SENTINEL
gh pr merge 1 --merge --delete-branch --subject \"Merge: x\" --body \"見出しなし\""
out=$(run_hook pre-merge-check "$DECOY_MERGE")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## Why"

# ===========================================================================
# pre-merge-check: head が base より遅れていれば deny（origin/main にリベースしてからマージする）
# ===========================================================================

it "pre-merge-check: base より遅れていれば deny し、リベースを求める"
make_fake_gh_merge 'echo 2'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "2 コミット"
assert_contains "$(reason "$out")" "リベース"

it "pre-merge-check: base 名に / があっても compare のパスとして正しく渡す（URL エンコード）"
make_fake_gh '"pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"release/x\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo "{\"name\":\"ci\",\"status\":\"completed\",\"conclusion\":\"success\"}" ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/release%2Fx...abc"*) echo 0 ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*) echo abc ;;
  "api --paginate repos/example/heredoc/pulls/1/files"*) true ;;
  "api graphql"*) echo "[]" ;;'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: base との差を取得できなければ deny"
make_fake_gh_merge 'echo "error connecting to api.github.com" >&2; exit 1'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "確認できません"

# ===========================================================================
# block-secret-commit: グローバルの git の hook を使わないリポジトリの git commit を gitleaks で止める
# ===========================================================================
# 機密情報の検査は git の pre-commit で行う（ADR-0015）。ローカルの core.hooksPath（husky など）を持つ
# リポジトリではグローバルの hook が走らないので、そこでだけ PreToolUse で検査する。対象のディレクトリを
# 決められない（無い、変数を含む）ときは、git の hook に任せて通す（#281）。検出は gitleaks に委ねるので、
# 偽の gitleaks で呼び出しと結果の扱いを確かめる

GLOBAL_HOOKS="$TEST_ROOT/global-hooks"
mkdir -p "$GLOBAL_HOOKS"
git config --global core.hooksPath "$GLOBAL_HOOKS"
REPO="$TEST_ROOT/secret-global"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
HUSKY="$TEST_ROOT/secret-husky"
make_repo "$HUSKY"
git -C "$HUSKY" switch -q -c feat/x
git -C "$HUSKY" config core.hooksPath .husky
cd "$HUSKY" || exit 1
echo 'plain' > a.txt && git add a.txt

LEAK_JSON='[{"File":"conf.txt","StartLine":3,"RuleID":"aws-access-token","Secret":"REDACTED","Fingerprint":"conf.txt:aws-access-token:3"}]'
# fake_gitleaks <staged-json|ok|fail> <pre-commit-json|ok|fail>
#   JSON を渡すと exit 1（漏えいあり）、"ok" なら [] と exit 0、"fail" なら JSON なしで exit 2
fake_gitleaks() {
  local body="" mode
  for mode in "--staged:$1" "--pre-commit:$2"; do
    local flag="${mode%%:*}" spec="${mode#*:}"
    case "$spec" in
      ok)   body+="\"git $flag --no-banner --redact --report-format json --report-path - .\"*) echo '[]' ;;"$'\n' ;;
      fail) body+="\"git $flag --no-banner --redact --report-format json --report-path - .\"*) echo 'error: x' >&2; exit 2 ;;"$'\n' ;;
      *)    body+="\"git $flag --no-banner --redact --report-format json --report-path - .\"*) echo '$spec'; exit 1 ;;"$'\n' ;;
    esac
  done
  make_fake_tool gitleaks "$body"
}

it "block-secret-commit: グローバルの hook を使うリポジトリでは gitleaks を呼ばない（git の pre-commit が検査する）"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit "git -C $REPO commit -m 'chore: a'")
assert_eq allow "$(decision "$out")"
assert_eq "" "$(fake_log gitleaks)"

it "block-secret-commit: git の pre-commit に任せるリポジトリでも、展開を含むエスケープはここで止める（git 側は展開後の値しか見えない）"
out=$(run_hook block-secret-commit "REASON=abcdef; ALLOW_SECRET_COMMIT=\$REASON git -C $REPO commit -m 'chore: a'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ALLOW_SECRET_COMMIT には理由を記載"
out=$(run_hook block-secret-commit "ALLOW_SECRET_COMMIT=\"\$(cat r.txt)\" git -C $REPO commit -m 'chore: a'")
assert_eq deny "$(decision "$out")"
# 字面の短い値は git 側も同じ規則で止めるので、ここでは咎めない
out=$(run_hook block-secret-commit "ALLOW_SECRET_COMMIT=1 git -C $REPO commit -m 'chore: a'")
assert_eq allow "$(decision "$out")"

it "block-secret-commit: ローカルの core.hooksPath を持つリポジトリでは、漏えいで止め、場所とルール ID を示す"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit 'git commit -m "chore: conf"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "conf.txt:3"
assert_contains "$(reason "$out")" "aws-access-token"
assert_not_contains "$(reason "$out")" "REDACTED"
fake_gitleaks ok ok
out=$(run_hook block-secret-commit 'git commit -m "chore: a"')
assert_eq allow "$(decision "$out")"

it "block-secret-commit: -a 指定時は未ステージの変更（--pre-commit）も検査する"
fake_gitleaks ok "$LEAK_JSON"
out=$(run_hook block-secret-commit 'git commit -am "chore: a"')
assert_eq deny "$(decision "$out")"
assert_contains "$(fake_log gitleaks)" "git --pre-commit"
fake_gitleaks ok "$LEAK_JSON"
out=$(run_hook block-secret-commit 'git commit -m"add a"')
assert_eq allow "$(decision "$out")"
assert_not_contains "$(fake_log gitleaks)" "git --pre-commit"
fake_gitleaks ok "$LEAK_JSON"
out=$(run_hook block-secret-commit 'git commit -m --all')
assert_eq allow "$(decision "$out")"
assert_not_contains "$(fake_log gitleaks)" "git --pre-commit"

it "block-secret-commit: 対象のディレクトリを決められないときは、git の hook に任せて通す（#281）"
fake_gitleaks "$LEAK_JSON" ok
for cmd in 'git -C $d commit -m "chore: a"' "git -C $TEST_ROOT/not-yet commit -m 'chore: a'" \
  "mkdir $TEST_ROOT/new && cd $TEST_ROOT/new && git init -q && git commit -m 'chore: a'"; do
  out=$(run_hook block-secret-commit "$cmd")
  assert_eq allow "$(decision "$out")"
done
assert_eq "" "$(fake_log gitleaks)"

it "block-secret-commit: gitleaks が失敗・無いときは、理由を分けて止める"
fake_gitleaks fail ok
out=$(run_hook block-secret-commit 'git commit -m "chore: x"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "gitleaks が失敗しました"
remove_fake_tool gitleaks
out=$(PATH="$(minimal_path)" run_hook block-secret-commit 'git commit -m "chore: x"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "gitleaks が PATH にありません"

it "block-secret-commit: ALLOW_SECRET_COMMIT=1 は検査を実行し、漏えいがあれば本来の文言に理由を求める行を足す"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit 'ALLOW_SECRET_COMMIT=1 git commit -m "chore: conf"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "conf.txt:3"
assert_contains "$(reason "$out")" "ALLOW_SECRET_COMMIT には理由を記載"
assert_contains "$(fake_log gitleaks)" "--staged"
fake_gitleaks ok ok
out=$(run_hook block-secret-commit 'ALLOW_SECRET_COMMIT=1 git commit -m "chore: a"')
assert_eq allow "$(decision "$out")"

it "block-secret-commit: ALLOW_SECRET_COMMIT=<理由> で迂回できる（gitleaks を呼ばない）"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit 'ALLOW_SECRET_COMMIT=テスト用 git commit -m "chore: conf"')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(fake_log gitleaks)"
git config --global --unset core.hooksPath

# ===========================================================================
# block-no-verify: git の hook を飛ばす git commit / git merge を止める
# ===========================================================================
# 機密情報の検査（gitleaks）は git の pre-commit で、ローカルのマージの検査は pre-merge-commit で行う
# （ADR-0015）。--no-verify（省略形と -n を含む）と、git -c core.hooksPath= はそれを飛ばすので止める

REPO="$TEST_ROOT/noverify"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1

it "block-no-verify: hook を飛ばす git commit / merge / pull と、hook の置き場所や CLAUDECODE の書き換えを止める"
for cmd in 'git commit --no-verify -m "chore: a"' 'git commit --no-verif -m "chore: a"' 'git commit -n -m "chore: a"' \
  'git commit -nm "chore: a"' 'git -C . commit -anm "chore: a"' 'git -c core.hooksPath=/dev/null commit -m "chore: a"' \
  'git -c core.hookspath=/x commit -m "chore: a"' 'git -C . -c core.hooksPath=/x commit -m "chore: a"' \
  'git merge --no-ff --no-verify feat/x' 'git merge --no-ve feat/x' 'git pull --no-verify origin feat/x' \
  'git pull -j --no-verify origin feat/x' 'git pull --jobs --no-verify' 'git --attr-source HEAD commit -n -m x' \
  'git -c core.hooksPath=/x pull' 'git config core.hooksPath /dev/null' 'git config --local core.hookspath x' \
  'git config --global --unset core.hooksPath' 'git config --unset-all core.hooksPath' \
  'git config --global --remove-section core' 'git config --rename-section core disabled' \
  'git config unset core.hooksPath' 'git config set core.hooksPath /x' 'git config remove-section core' \
  'git config --global rename-section core x' \
  'git --config-env=core.hooksPath=H commit -m x' 'CLAUDECODE= git commit -m x' 'CLAUDECODE=0 git commit -m x' \
  'env -u CLAUDECODE git commit -m x' 'unset CLAUDECODE && git commit -m x' \
  'env CLAUDECODE=0 git commit -m x' 'env -i git commit -m x' 'env FOO=1 git commit --no-verify -m x' \
  'git -c core.hooksPath commit -m x' 'git --config-env core.hooksPath commit -m x' \
  'command git commit --no-verify -m x' 'env GIT_CONFIG_COUNT=1 git commit -m x' \
  "env -S 'git commit --no-verify -m x'" 'exec -c git commit -m x' 'exec -cl git commit -m x' \
  'env -vi git commit -m x' 'env --ignore-env git commit -m x' 'env -iu X git commit -m x' \
  'env --uns=CLAUDECODE git commit -m x' 'nice git commit --no-verify -m x' 'nice -n 10 git commit -n -m x' \
  'timeout 60 git commit --no-verify -m x' 'timeout -k 5 60 git commit -n -m x' 'setsid git commit -n -m x' \
  'stdbuf -oL git commit --no-verify -m x' 'ionice -c3 git commit --no-verify -m x' \
  'env -S "git\_commit\_--no-verify" -m x' 'env -- FOO=1 git commit --no-verify -m x' \
  "env -S '# note' git commit --no-verify -m x" \
  'env -- CLAUDECODE=0 git commit -m x' 'builtin command git commit --no-verify -m x' \
  'GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/x git commit -m x'; do
  out=$(run_hook block-no-verify "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "ALLOW_NO_VERIFY=<理由>"
done

it "block-no-verify: sudo、xargs、flock、ssh、watch、eval で包んだ git と、bash -c の文字列の中の git も見る"
for cmd in 'sudo -E git commit --no-verify -m x' 'sudo -Eu me git commit -n -m x' 'xargs -0 git commit -n -m x' \
  'flock /tmp/l git commit --no-verify -m x' 'ssh host git commit --no-verify -m x' 'watch git commit -n -m x' \
  'eval "git commit --no-verify -m x"' 'eval git commit -n -m x' "bash -c 'git commit --no-verify -m x'" \
  "bash -x -c 'git commit -n -m x'" "sh -c 'CLAUDECODE=0 git commit -m x'" "bash -c 'CLAUDECODE=0; git commit -m x'" \
  "bash -c 'unset CLAUDECODE; git commit -m x'" "env -u CLAUDECODE bash -c 'git commit -m x'" \
  "bash -c 'git -c core.hooksPath=/x commit -m x'" 'sudo -E GIT_CONFIG_COUNT=1 git commit -m x' \
  'flock /tmp/l -c "git commit -n -m x"' 'script -c "git commit --no-verify -m x" log.txt'; do
  out=$(run_hook block-no-verify "$cmd")
  assert_eq deny "$(decision "$out")"
done
# sudo は -E が無ければ環境変数を受け継がず（env_reset）、ssh 先には環境変数が渡らないので、CLAUDECODE が
# 消える（hook が Claude Code のセッションと分からない）
for cmd in 'sudo git commit -m x' 'sudo --preserve-env=HOME git commit -m x' 'ssh host git commit -m x'; do
  out=$(run_hook block-no-verify "$cmd")
  assert_eq deny "$(decision "$out")"
done
for cmd in 'sudo -E git commit -m x' 'sudo --preserve-env git commit -m x' \
  "bash -c 'git commit -m x'" 'eval git commit -m x' 'flock /tmp/l git commit -m x' \
  'sudo systemctl restart x' 'ssh r995 nvidia-smi' 'env -i ls' 'sudo -D /tmp ls' 'ssh r995 git status' \
  "bash -c 'CLAUDECODE=0 echo ok'" 'CLAUDECODE=0 echo ok' 'unset CLAUDECODE; echo ok'; do
  out=$(run_hook block-no-verify "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "block-no-verify: env の - は getopt の後に一度だけ（-- の後でも）読む"
# `env -- - git` は空の環境で git を実行する。`env - -u X git` は -u を、`env - - git` は - を実行する
out=$(run_hook block-no-verify 'env -- - git commit -m x')
assert_eq deny "$(decision "$out")"
for cmd in 'env - -u CLAUDECODE git commit -m x' 'env - - git commit -m x' 'env - -- git commit -n -m x'; do
  out=$(run_hook block-no-verify "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "block-no-verify: hook を飛ばさない git のコマンドは許可する"
for cmd in 'git commit -m "chore: a"' 'git commit -am "chore: a"' 'git commit --amend --no-edit' \
  'git commit -m"feat: add notes"' 'git commit -Fnotes.txt' 'git commit -Cnext' 'git commit --no-status -m x' \
  'git merge --no-ff feat/x' 'git -c user.name=n commit -m x' 'git commit -m -nightly' \
  'git commit -m "-nightly build fix"' 'git commit --author -n -m x' 'git config --get core.hooksPath' \
  'git config core.autocrlf false' 'git config --remove-section alias' 'git config get core.hooksPath' \
  'env FOO=1 git commit -m x' 'env CLAUDECODE=1 git commit -m x' "env -S \"git commit -m 'x -n'\"" \
  'git commit -m --no-verify' 'git merge -m --no-verify feat/x' 'env -i CLAUDECODE=1 git commit -m x' \
  'git pull origin main'; do
  out=$(run_hook block-no-verify "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "block-no-verify: ALLOW_NO_VERIFY=<理由> で迂回できる"
out=$(run_hook block-no-verify 'ALLOW_NO_VERIFY=テスト用 git commit --no-verify -m "chore: a"')
assert_eq allow "$(decision "$out")"

it "block-no-verify: git commit / merge 以外とヒアドキュメント本文には反応しない"
out=$(run_hook block-no-verify 'git log --no-verify')
assert_eq "" "$out"
out=$(run_hook block-no-verify "$(write_doc '例: git commit --no-verify -m msg')")
assert_eq "" "$out"

# ===========================================================================
# guard-git-add: 対象を絞らない git add（-A / --all / . / :/ / *）を止める
# ===========================================================================
# 1 コミット 1 論理変更と、機密情報の混入防止（git の pre-commit の機密情報の検査と対。ADR-0015）のため。
# パスで範囲を限定した -A（git add -A src/）と、追跡済みだけを対象にする -u は通す

REPO="$TEST_ROOT/gitadd"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1
mkdir -p src && echo x > src/a.txt

it "guard-git-add: パスを指定した git add は許可する"
for cmd in 'git add src/a.txt' 'git add -p' 'git add -u' 'git add ./src' 'git add .claude/' 'git add -- src/a.txt' \
  'git add --pathspec-from-file=paths.txt'; do
  out=$(run_hook guard-git-add "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-git-add: -A / --all / . / :/ / * は deny し、代わりの手順を示す"
for cmd in 'git add -A' 'git add --all' 'git add .' 'git add -A .' 'git add :/' 'git add *' 'git add -An' \
  'git add -A --pathspec-from-file paths.txt' 'git add -A --pathspec-from-file=paths.txt' \
  'env FOO=1 git add -A' 'command git add .' 'sudo git add -A' "bash -c 'git add .'" 'eval git add -A' \
  'ssh host "cd repo && git add -A"'; do
  out=$(run_hook guard-git-add "$cmd")
  assert_eq deny "$(decision "$out")"
done
assert_contains "$(reason "$out")" "git add -p"

it "guard-git-add: 文字列引数の中の git add -A はコマンドではない（bash 版の誤検出）"
out=$(run_hook guard-git-add 'git commit -m "docs: run git add -A"')
assert_eq allow "$(decision "$out")"

it "guard-git-add: パスで範囲を限定した -A は許可する"
out=$(run_hook guard-git-add 'git add -A src/')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-add 'git add --all -- src/')
assert_eq allow "$(decision "$out")"

it "guard-git-add: git add 以外は対象外"
out=$(run_hook guard-git-add 'git commit -am "feat: x"')
assert_eq "" "$out"
out=$(run_hook guard-git-add 'git log --grep "add -A"')
assert_eq "" "$out"

it "guard-git-add: 連結コマンドの 2 つ目以降も検査する"
out=$(run_hook guard-git-add 'git add src/a.txt && git add -A')
assert_eq deny "$(decision "$out")"

it "guard-git-add: -C で指定した別リポジトリでも検査する"
cd "$TEST_ROOT" || exit 1
out=$(run_hook guard-git-add "git -C $REPO add -A")
assert_eq deny "$(decision "$out")"
cd "$REPO" || exit 1

it "guard-git-add: ALLOW_GIT_ADD_ALL=<理由> で迂回できる"
out=$(run_hook guard-git-add 'ALLOW_GIT_ADD_ALL=テスト用 git add -A')
assert_eq allow "$(decision "$out")"

it "guard-git-add: ヒアドキュメント本文の git add -A には反応しない"
out=$(run_hook guard-git-add "$(write_doc 'git add -A は禁止')")
assert_eq allow "$(decision "$out")"

# ===========================================================================
# guard-gh-run-rerun: 既に再実行済みの run の gh run rerun を止める
# ===========================================================================
# 一時障害の再実行は 1 回まで。attempt 2 以上で同じ失敗なら一時障害ではないので、
# 無制限に再実行せず原因を報告してユーザーの判断に委ねる

cd "$TEST_ROOT" || exit 1

it "guard-gh-run-rerun: 初回の失敗（attempt 1）の再実行は許可する"
make_fake_gh '"run view 100 --json attempt"*) echo "{\"attempt\":1}" ;;'
out=$(run_hook guard-gh-run-rerun 'gh run rerun 100')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-gh-run-rerun 'gh run rerun 100 --failed')
assert_eq allow "$(decision "$out")"

it "guard-gh-run-rerun: attempt 2 以上の run は deny し、報告に切り替えるよう示す"
make_fake_gh '"run view 100 --json attempt"*) echo "{\"attempt\":2}" ;;'
out=$(run_hook guard-gh-run-rerun 'gh run rerun 100')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "2 回"
out=$(run_hook guard-gh-run-rerun 'gh run rerun --failed 100')
assert_eq deny "$(decision "$out")"

it "guard-gh-run-rerun: -R 指定のリポジトリで attempt を確認する"
make_fake_gh '"run view -R octo/repo 100 --json attempt"*) echo "{\"attempt\":3}" ;;'
out=$(run_hook guard-gh-run-rerun 'gh run rerun -R octo/repo 100')
assert_eq deny "$(decision "$out")"

it "guard-gh-run-rerun: --job の値（数値のジョブ ID）を run ID と取り違えない"
make_fake_gh '"run view 100 --json attempt"*) echo "{\"attempt\":1}" ;;'
out=$(run_hook guard-gh-run-rerun 'gh run rerun --job 555 100')
assert_eq allow "$(decision "$out")"
assert_contains "$(cat "$FAKE_GH_LOG")" "run view 100 --json attempt"
out=$(run_hook guard-gh-run-rerun 'gh run rerun 100 -j 555')
assert_eq allow "$(decision "$out")"

it "guard-gh-run-rerun: attempt を確認できなければ deny"
make_fake_gh '"run view 100 --json attempt"*) echo "not found" >&2; exit 1 ;;'
out=$(run_hook guard-gh-run-rerun 'gh run rerun 100')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "確認できません"

it "guard-gh-run-rerun: ssh と sudo -i の中の gh run rerun は、その中で走るので手元で確かめられないと示して止める（gh を呼ばない）"
make_fake_gh ''
out=$(run_hook guard-gh-run-rerun 'ssh host gh run rerun 100')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ssh や sudo -i の中で走る gh run rerun は、手元の場所で試行回数を確かめられないため止めます"
out=$(run_hook guard-gh-run-rerun 'sudo -i gh run rerun 100')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ssh や sudo -i の中で走る gh run rerun は、手元の場所で試行回数を確かめられないため止めます"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-gh-run-rerun: gh run rerun 以外は対象外で gh を呼ばない"
make_fake_gh ''
out=$(run_hook guard-gh-run-rerun 'gh run view 100 --log-failed')
assert_eq "" "$out"
out=$(run_hook guard-gh-run-rerun 'gh run list')
assert_eq "" "$out"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-gh-run-rerun: ALLOW_RERUN=<理由> で迂回できる（gh を呼ばない）"
out=$(run_hook guard-gh-run-rerun 'ALLOW_RERUN=テスト用 gh run rerun 100')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-gh-run-rerun: ヒアドキュメント本文の gh run rerun には反応しない"
out=$(run_hook guard-gh-run-rerun "$(write_doc '再実行: gh run rerun 100')")
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

# ===========================================================================
# guard-gh-api: 生の gh api で行ってはいけない操作を止める
# ===========================================================================
# 1. Actions の権限設定（actions/permissions 配下）への書き込み。CI の権限エラーを
#    リポジトリ設定 default_workflow_permissions の緩和で回避させない
# 2. GraphQL の resolveReviewThread / unresolveReviewThread。resolve は resolve-thread.sh
#    経由に固定し、人間のレビュアーのスレッドを勝手に閉じない判定を迂回できなくする

it "guard-gh-api: actions/permissions の読み取り（GET）は許可する"
out=$(run_hook guard-gh-api 'gh api repos/octo/repo/actions/permissions/workflow')
assert_eq allow "$(decision "$out")"

it "guard-gh-api: actions/permissions への書き込みは deny し、permissions: の宣言を示す"
out=$(run_hook guard-gh-api 'gh api -X PUT repos/octo/repo/actions/permissions/workflow -f default_workflow_permissions=write')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "permissions:"
out=$(run_hook guard-gh-api 'gh api --method PATCH repos/octo/repo/actions/permissions')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-gh-api 'gh api repos/octo/repo/actions/permissions/workflow -f default_workflow_permissions=write')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-gh-api 'gh api "repos/{owner}/{repo}/actions/permissions/workflow" --input body.json')
assert_eq deny "$(decision "$out")"

it "guard-gh-api: GraphQL の resolveReviewThread は deny し、resolve-thread.sh を示す"
out=$(run_hook guard-gh-api "gh api graphql -f query='mutation { resolveReviewThread(input: {threadId: \"x\"}) { thread { id } } }'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "resolve-thread.sh <PR の URL> <comment_id>"
out=$(run_hook guard-gh-api "gh api graphql -f query='mutation { unresolveReviewThread(input: {threadId: \"x\"}) { thread { id } } }'")
assert_eq deny "$(decision "$out")"

it "guard-gh-api: ヒアドキュメントで渡した mutation も deny する（本文が操作そのもの）"
out=$(run_hook guard-gh-api "gh api graphql -f query=\"\$(cat <<'EOF'
mutation { resolveReviewThread(input: {threadId: \"x\"}) { thread { id } } }
EOF
)\"")
assert_eq deny "$(decision "$out")"

it "guard-gh-api: reviewThreads の読み取りクエリは許可する"
out=$(run_hook guard-gh-api "gh api graphql -f query='query { repository(owner: \"o\", name: \"r\") { pullRequest(number: 1) { reviewThreads(first: 100) { nodes { id isResolved } } } } }'")
assert_eq allow "$(decision "$out")"

it "guard-gh-api: resolve-thread.sh の実行と gh api 以外は対象外"
# shellcheck disable=SC2088  # 展開前のコマンド文字列をそのまま hook に渡す
out=$(run_hook guard-gh-api '~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh 1 2')
assert_eq "" "$out"
out=$(run_hook guard-gh-api 'gh pr view 1')
assert_eq "" "$out"

it "guard-gh-api: 説明文の中の resolveReviewThread や actions/permissions には反応しない"
out=$(run_hook guard-gh-api "$(write_doc 'resolveReviewThread は gh api で叩かない。actions/permissions も PUT しない')")
assert_eq allow "$(decision "$out")"

# ===========================================================================
# pre-merge-check: 最後の push が自動レビューを受けているか
# ===========================================================================
# push しても再レビューは走らない（依頼して初めて走る）。依頼を忘れると
# 「レビューされていない版」をマージできてしまうため、bot レビューが見た
# commit と head SHA の一致を機械的に確かめる

it "pre-merge-check: 最後の push がレビューされていなければ deny し、再レビュー依頼を促す"
make_fake_gh_merge 'echo 0' 'echo old'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "レビュー"
# PR のスクリプトは対象を URL で受ける（ADR-0032）
assert_contains "$(reason "$out")" "request-rereview.sh https://github.com/example/heredoc/pull/1 "
assert_contains "$(reason "$out")" "/gh-actions-check https://github.com/example/heredoc/pull/1 で状況を診断"

it "pre-merge-check: bot のレビューが head を見ていればマージできる"
make_fake_gh_merge 'echo 0' 'echo abc'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: bot のレビューが 1 件も無いリポジトリではこの検査をしない"
make_fake_gh_merge 'echo 0' 'echo ""'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: レビューの実行が失敗していたら、CI 失敗ではなくレビュー未実施として報告する"
# トークン枯渇や内部エラーで Copilot のチェックが落ちた場合、コードの問題ではない。
# CI 失敗として止めると、レビューが回らない間まったくマージできなくなる
make_fake_gh '"pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo "{\"name\":\"copilot-pull-request-reviewer\",\"status\":\"completed\",\"conclusion\":\"failure\"}" ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/main...abc"*) echo 0 ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*) echo old ;;
  "api --paginate repos/example/heredoc/pulls/1/files"*) true ;;
  "api graphql"*) echo "[]" ;;'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "レビューが実行されていません"
assert_contains "$(reason "$out")" "ALLOW_UNREVIEWED_HEAD=<理由>"
assert_not_contains "$(reason "$out")" "失敗したチェックがあります"
# 診断のスキルにも対象を URL で渡す（ADR-0032）
assert_contains "$(reason "$out")" "/gh-actions-check https://github.com/example/heredoc/pull/1 "

it "pre-merge-check: レビューが 1 件も無くてもチェックが失敗していれば止める"
# 初回レビューがトークン枯渇で落ちた PR。レビュー 0 件なので commit_id の比較では
# 検出できないが、レビュー未実施であることに変わりはない
make_fake_gh '"pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo "{\"name\":\"copilot-pull-request-reviewer\",\"status\":\"completed\",\"conclusion\":\"failure\"}" ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/main...abc"*) echo 0 ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*) echo "" ;;
  "api --paginate repos/example/heredoc/pulls/1/files"*) true ;;
  "api graphql"*) echo "[]" ;;'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "レビューが実行されていません"

it "pre-merge-check: ALLOW_UNREVIEWED_HEAD=<理由> なら未レビューの head でもマージできる"
out=$(run_hook pre-merge-check "ALLOW_UNREVIEWED_HEAD=テスト用 $MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: ALLOW_UNREVIEWED_HEAD=1 は、本来の文言に理由を求める行を足して止める"
out=$(run_hook pre-merge-check "ALLOW_UNREVIEWED_HEAD=1 $MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "レビューが実行されていません"
assert_contains "$(reason "$out")" "ALLOW_UNREVIEWED_HEAD には理由を記載"

# リベース（main の追従）で hash だけが変わった head は、レビュー済みの差分と同じなので再レビューを求めない。
# 差分が変わっていれば今まで通り deny する（変わったのに通す誤りを許さない）
REBASE_REPO="$TEST_ROOT/rebase-repo"
git init -q -b main "$REBASE_REPO"
git -C "$REBASE_REPO" config user.email t@example.com
git -C "$REBASE_REPO" config user.name t
git -C "$REBASE_REPO" config core.hooksPath /dev/null
git -C "$REBASE_REPO" config commit.gpgsign false
echo base >"$REBASE_REPO/f.txt"
git -C "$REBASE_REPO" add f.txt
git -C "$REBASE_REPO" commit -q -m base
git -C "$REBASE_REPO" update-ref refs/remotes/origin/main HEAD
git -C "$REBASE_REPO" checkout -q -b pr
echo pr >"$REBASE_REPO/pr.txt"
git -C "$REBASE_REPO" add pr.txt
git -C "$REBASE_REPO" commit -q -m pr
REBASE_OLD=$(git -C "$REBASE_REPO" rev-parse HEAD)
git -C "$REBASE_REPO" checkout -q main
echo more >"$REBASE_REPO/g.txt"
git -C "$REBASE_REPO" add g.txt
git -C "$REBASE_REPO" commit -q -m "main moves"
git -C "$REBASE_REPO" update-ref refs/remotes/origin/main HEAD
REBASE_TIP=$(git -C "$REBASE_REPO" rev-parse HEAD)
git -C "$REBASE_REPO" checkout -q -b pr-rebased
echo pr >"$REBASE_REPO/pr.txt"
git -C "$REBASE_REPO" add pr.txt
git -C "$REBASE_REPO" commit -q -m "pr (rebased)"
REBASE_SAME=$(git -C "$REBASE_REPO" rev-parse HEAD)
echo changed >"$REBASE_REPO/pr.txt"
git -C "$REBASE_REPO" add pr.txt
git -C "$REBASE_REPO" commit -q -m "pr changes content"
REBASE_CHANGED=$(git -C "$REBASE_REPO" rev-parse HEAD)

make_fake_gh_rebased() {
  # $1: 今の head、$2: bot がレビューした commit、$3: ローカルレビューの記録の 1 行目を出力するコマンド（既定は記録なし）
  make_fake_gh '"pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"'"$1"'\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/'"$1"'/check-runs"*) echo "{\"name\":\"ci\",\"status\":\"completed\",\"conclusion\":\"success\"}" ;;
  "api repos/example/heredoc/commits/'"$1"'/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/main...'"$1"'"*) echo 0 ;;
  "api repos/example/heredoc/branches/main"*) echo '"$REBASE_TIP"' ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*) echo '"$2"' ;;
  "api --paginate repos/example/heredoc/pulls/1/files"*) true ;;
  "api --paginate repos/example/heredoc/issues/1/comments"*) '"${3:-true}"' ;;
  "api repos/example/heredoc/pulls/1 "*) echo main ;;
  "api graphql"*) echo "[]" ;;'
}

it "pre-merge-check: リベースだけで head が変わり、PR の差分がレビュー済みと同じなら再レビューを求めない"
cd "$REBASE_REPO" || exit 1
make_fake_gh_rebased "$REBASE_SAME" "$REBASE_OLD"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: リベース後に PR の中身が変わっていれば deny する"
make_fake_gh_rebased "$REBASE_CHANGED" "$REBASE_OLD"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"

it "pre-merge-check: レビュー済みのコミットを取得できなければ deny する"
make_fake_gh_rebased "$REBASE_SAME" "0123456789012345678901234567890123456789"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

# ローカルレビューの記録の後に rebase だけした head も、記録の SHA を比較元にして同じ判定で通す（ADR-0059）。
# bot のレビューは手元に無いコミットを指し、bot の側からは確かめられないようにする
BOGUS_SHA=0123456789012345678901234567890123456789
local_record() { printf "printf '%%s\\n' '<!-- local-review sha=%s high=%s medium=0 low=0 -->'" "$1" "$2"; }

it "pre-merge-check: ローカルレビューの記録の SHA から rebase だけした head は通す"
make_fake_gh_rebased "$REBASE_SAME" "$BOGUS_SHA" "$(local_record "$REBASE_OLD" 0)"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: ローカルレビューの記録の後に差分を変えた head は止める"
make_fake_gh_rebased "$REBASE_CHANGED" "$BOGUS_SHA" "$(local_record "$REBASE_OLD" 0)"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: High のあるローカルレビューの記録は比較元にしない"
make_fake_gh_rebased "$REBASE_SAME" "$BOGUS_SHA" "$(local_record "$REBASE_OLD" 1)"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: 最後の記録に High があれば、前の High の無い記録も比較元にしない"
# 同じ差分を回し直して見つけた High を、前の記録で隠さない
make_fake_gh_rebased "$REBASE_SAME" "$BOGUS_SHA" \
  "$(local_record "$REBASE_OLD" 0); $(local_record "$BOGUS_SHA" 1)"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

# local-review-covers: decide-next.sh と request-rereview.sh が hook と同じ判定を使う入口。覆われていれば 0、いなければ 1、
# 引数の数の誤りは 2、記録を取得できなければ 3。rebase だけかは hook と同じく cwd の clone で比べる。
# PR の worktree に近い形として、origin は GitHub の URL にし、insteadOf で手元の REBASE_REPO から取らせる（ネットワークに出ない）
COVERS_REPO="$TEST_ROOT/covers-repo"
git clone -q "$REBASE_REPO" "$COVERS_REPO"
git -C "$COVERS_REPO" remote set-url origin git@github.com:example/heredoc.git
git -C "$COVERS_REPO" config "url.$REBASE_REPO.insteadOf" git@github.com:example/heredoc.git
# covers <cwd> <head> <記録のコマンド>: 終了コード
covers() {
  make_fake_gh_rebased "$2" "$BOGUS_SHA" "$3"
  (cd "$1" && "$CLAUDE_HOOKS_BIN" local-review-covers example/heredoc 1 "$2" 2>/dev/null)
  echo $?
}

it "local-review-covers: 記録の SHA から rebase だけした head は終了コード 0"
assert_eq 0 "$(covers "$COVERS_REPO" "$REBASE_SAME" "$(local_record "$REBASE_OLD" 0)")"

it "local-review-covers: head の記録があれば終了コード 0"
assert_eq 0 "$(covers "$TEST_ROOT" "$REBASE_SAME" "$(local_record "$REBASE_SAME" 0)")"

it "local-review-covers: 記録の後に差分を変えた head は終了コード 1"
assert_eq 1 "$(covers "$COVERS_REPO" "$REBASE_CHANGED" "$(local_record "$REBASE_OLD" 0)")"

it "local-review-covers: 最後の記録に High があれば比べずに終了コード 1"
assert_eq 1 "$(covers "$COVERS_REPO" "$REBASE_SAME" "$(local_record "$REBASE_OLD" 0); $(local_record "$BOGUS_SHA" 1)")"

it "local-review-covers: cwd の origin の URL を問わず、手元のコミットで hook と同じく比べる"
# REBASE_REPO には rebase の前後のコミットがあるが origin が無い。fork や ssh の別名の origin でも hook と同じ判定にする
assert_eq 0 "$(covers "$REBASE_REPO" "$REBASE_SAME" "$(local_record "$REBASE_OLD" 0)")"

it "local-review-covers: 新しい head が手元に無ければ、旧 head と同じく origin から取って比べる"
# 別の clone から push した head を、まだ fetch していない clone で判定する場合
PARTIAL_REPO="$TEST_ROOT/partial-repo"
git clone -q --no-local --single-branch -b pr "$REBASE_REPO" "$PARTIAL_REPO"
assert_eq 0 "$(covers "$PARTIAL_REPO" "$REBASE_SAME" "$(local_record "$REBASE_OLD" 0)")"

# 記録の読み取りの jq を、偽 gh で本物の jq にかけて確かめる（書き込める人の、1 行目の記録だけを数える）
# covers_json <JSON>: コメントの配列を fixture にして local-review-covers の終了コードを出す
covers_json() {
  printf '%s' "$1" >"$TEST_ROOT/covers-comments.json"
  covers "$COVERS_REPO" "$REBASE_SAME" "shift 3; jq -r \"\$2\" $TEST_ROOT/covers-comments.json"
}
REC_OLD="<!-- local-review sha=$REBASE_OLD high=0 medium=0 low=0 -->"
REC_HIGH="<!-- local-review sha=$BOGUS_SHA high=1 medium=0 low=0 -->"

it "local-review-covers: 書き込める人の記録は数える"
assert_eq 0 "$(covers_json "[{\"author_association\":\"OWNER\",\"body\":\"$REC_OLD\"}]")"

it "local-review-covers: 書き込めない人の記録は数えない"
assert_eq 1 "$(covers_json "[{\"author_association\":\"NONE\",\"body\":\"$REC_OLD\"}]")"

it "local-review-covers: 書き込めない人の High の記録は、最後の記録に数えない"
assert_eq 0 "$(covers_json "[{\"author_association\":\"OWNER\",\"body\":\"$REC_OLD\"},{\"author_association\":\"NONE\",\"body\":\"$REC_HIGH\"}]")"

it "local-review-covers: コメントの 2 行目以降の記録は数えない"
assert_eq 1 "$(covers_json "[{\"author_association\":\"OWNER\",\"body\":\"note\\n$REC_OLD\"}]")"

it "local-review-covers: 記録を取得できなければ終了コード 3"
assert_eq 3 "$(covers "$COVERS_REPO" "$REBASE_SAME" 'exit 1')"

it "local-review-covers: head が 16 進の SHA でなければ（git の引数に渡さない）終了コード 3"
assert_eq 3 "$(covers "$COVERS_REPO" "--upload-pack=touch" "$(local_record "$REBASE_OLD" 0)")"

it "local-review-covers: 引数の数が違えば終了コード 2"
"$CLAUDE_HOOKS_BIN" local-review-covers example/heredoc 1 2>/dev/null
assert_eq 2 $?
cd "$REPO" || exit 1

it "pre-merge-check: レビューは走っているのに未レビューなら、まず要求を促す"
make_fake_gh_merge 'echo 0' 'echo old'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"

it "pre-merge-check: 未レビューの案内は、再レビューの依頼と理由つきのエスケープだけで、条件を列挙しない"
# 「〜なら付けてよい」の条件を並べると、どれかに当てはめる読み方を誘う（ADR-0023）
make_fake_gh_merge 'echo 0' 'echo old'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"
assert_contains "$(reason "$out")" "ALLOW_UNREVIEWED_HEAD=<理由>"
assert_contains "$(reason "$out")" "issue"
assert_not_contains "$(reason "$out")" "挙動を変えない"
assert_not_contains "$(reason "$out")" "ユーザーが承認"
assert_not_contains "$(reason "$out")" "周回上限"

# 記録があれば再レビューではなくローカルレビューの記録で終える（ADR-0043）。条件は並べない
# High が残らなければ再レビューではなくローカルレビューの記録で終える（ADR-0043）。条件は並べない
make_fake_gh_merge 'echo 0' 'echo old'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_contains "$(reason "$out")" "decide-next.sh https://github.com/example/heredoc/pull/1"

# head にローカルレビューの記録（High なし）があれば、Copilot のレビューの有無・到着・内容を問わず通す
# （ADR-0043）。Copilot のレビューが来ていればその指摘への対応（未解決スレッド）は検査 6 が要る

# make_fake_gh_merge_local <comments> [check_run] [reviewed]
# comments は、書き込める人の記録（ローカルレビューの記録）の本文の 1 行目を出力するコマンド。check_run は head のチェック 1 件の JSON（既定は CI の成功）、reviewed は bot レビューの
# 対象コミットを出力するコマンド（既定は head と違う "old"）
make_fake_gh_merge_local() {
  local check_run="${2:-{\"name\":\"ci\",\"status\":\"completed\",\"conclusion\":\"success\"\}}"
  local reviewed="${3:-echo old}"
  make_fake_gh '"pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo '"'$check_run'"' ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/main...abc"*) echo 0 ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*) '"$reviewed"' ;;
  "api --paginate repos/example/heredoc/pulls/1/files"*) echo src/main.rs ;;
  "api --paginate repos/example/heredoc/issues/1/comments"*) '"$1"' ;;
  "api graphql"*) echo "[]" ;;'
}
LOCAL_ABC="printf '%s\n' '<!-- local-review sha=abc high=0 medium=1 low=0 -->'"

it "pre-merge-check: head のローカルレビューの記録があれば、最後の push が未レビューでもマージできる"
make_fake_gh_merge_local "$LOCAL_ABC"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: 記録は書き込みの権限を持つ人のコメントだけを受け付ける"
assert_contains "$(grep 'issues/1/comments' "$TEST_ROOT/gh.log")" "author_association"

it "pre-merge-check: Copilot のレビューが 1 件も無くても、Copilot のチェックが失敗していても、記録があればマージできる"
make_fake_gh_merge_local "$LOCAL_ABC" \
  '{"name":"copilot-pull-request-reviewer","status":"completed","conclusion":"failure"}' 'echo'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: Copilot のレビューの後に周回を数え直していても、記録があればマージできる"
make_fake_gh_merge_local \
  "printf '%s\n' '<!-- review-rounds-reset -->' '<!-- local-review sha=abc high=0 medium=0 low=0 -->'"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: head の記録が無ければ止める"
make_fake_gh_merge_local true
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: 記録が前の head のものなら止める"
make_fake_gh_merge_local \
  "printf '%s\n' '<!-- local-review sha=old high=0 medium=0 low=0 -->'"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: 記録の SHA が head の前方だけ一致しても止める"
make_fake_gh_merge_local \
  "printf '%s\n' '<!-- local-review sha=ab high=0 medium=0 low=0 -->'"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: head の記録に High があれば（直していない High が残る）止める"
make_fake_gh_merge_local \
  "printf '%s\n' '<!-- local-review sha=abc high=1 medium=0 low=0 -->'"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: 同じ head の後の記録に High があれば、前の High の無い記録があっても止める"
make_fake_gh_merge_local \
  "printf '%s\n' '<!-- local-review sha=abc high=0 medium=0 low=0 -->' '<!-- local-review sha=abc high=1 medium=0 low=0 -->'"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: 記録のコメントを取得できなければ止める"
make_fake_gh_merge_local 'exit 1'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

# 文書（docs/）だけの PR は、最初の自動レビューで指摘を受ければ足りる（ADR-0004）。
# 文章は細部をいくらでも掘れるので、push ごとに再レビューを求めると収束しない
# （nucrawler #145 は 23 コミット・80 コメント、#146 も指摘 0 件の後に毎回新しい論点が出た）
# make_fake_gh_merge_files <files> [reviewed] [check_run] [succeeded]
# files は PR の変更ファイル、reviewed は bot レビューの対象コミットを出力するコマンド
# （既定は head と違う "old"）、check_run は head のチェック 1 件の JSON（既定は CI の成功）、
# succeeded は失敗していない bot レビューの件数を出力するコマンド（既定は 1 件）
make_fake_gh_merge_files() {
  local files="$1" reviewed="${2:-echo old}" succeeded="${4:-echo 1}"
  local check_run="${3:-{\"name\":\"ci\",\"status\":\"completed\",\"conclusion\":\"success\"\}}"
  make_fake_gh '"pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo '"'$check_run'"' ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/main...abc"*) echo 0 ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*"able to review"*) '"$succeeded"' ;;
  "api --paginate repos/example/heredoc/pulls/1/reviews"*) '"$reviewed"' ;;
  "api --paginate repos/example/heredoc/pulls/1/files"*) '"$files"' ;;
  "api graphql"*) echo "[]" ;;'
}

it "pre-merge-check: 文書（docs/）だけの PR は、最後の push が未レビューでもマージできる"
make_fake_gh_merge_files 'printf "%s\n" docs/plans/010_x.md docs/plans/011_y.md'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: ADR（docs/adr/）だけの PR も、最後の push が未レビューでもマージできる"
make_fake_gh_merge_files 'printf "%s\n" docs/adr/0004-x.md docs/issues/y.md'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: 文書だけの PR は、一度レビューを受けていれば最後の push のレビューが失敗していてもマージできる"
make_fake_gh_merge_files 'echo docs/plans/010_x.md' 'echo old' \
  '{"name":"copilot-pull-request-reviewer","status":"completed","conclusion":"failure"}'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: 文書だけの PR でも、レビューが一度も成功していなければ止める"
make_fake_gh_merge_files 'echo docs/plans/010_x.md' 'echo ""' \
  '{"name":"copilot-pull-request-reviewer","status":"completed","conclusion":"failure"}'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "レビューが実行されていません"

it "pre-merge-check: 文書だけの PR でも、bot のレビューが失敗したものだけなら止める"
# Copilot はレビューできなかったときも本文だけのレビューを提出するので、
# レビューの有無ではなく、失敗していないレビューがあるかで判定する
make_fake_gh_merge_files 'echo docs/plans/010_x.md' 'echo old' '' 'echo 0'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"

it "pre-merge-check: docs/ 以外のファイルも含む PR は、これまでどおり再レビューを求める"
make_fake_gh_merge_files 'printf "%s\n" docs/plans/010_x.md src/main.rs'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"

it "pre-merge-check: docs/ に似た別の場所は文書として扱わない"
make_fake_gh_merge_files 'printf "%s\n" docs.md docs-extra/x.md src/docs/adr/x.md'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"

it "pre-merge-check: src/ から docs/ への移動は、旧パスも数えて文書だけとみなさない"
# files API は移動の新しいパスを filename、旧パスを previous_filename で返す。
# 旧パスを要求していれば src/foo.rs が見え、要求していなければ docs/ だけに見える
make_fake_gh_merge_files 'case "$*" in *previous_filename*) printf "%s\n" docs/foo.md src/foo.rs ;; *) echo docs/foo.md ;; esac'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"

it "pre-merge-check: 変更ファイルを取得できなければ、文書だけとみなさず再レビューを求める"
make_fake_gh_merge_files 'echo "error connecting to api.github.com" >&2; exit 1'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"

it "pre-merge-check: ssh と sudo -i の中の gh pr merge は、cd 先でなく、その中で走るので手元で検査できないと示して止める"
# 手元の場所の検査（マージ方式の指定など）は、接続先の PR に当たらないので理由に混ぜない
out=$(run_hook pre-merge-check "ssh host gh pr merge 1")
assert_eq deny "$(decision "$out")"
assert_eq "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます" "$(reason "$out")"
out=$(run_hook pre-merge-check "ssh host $MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます"
assert_not_contains "$(reason "$out")" "cd 先"
out=$(run_hook pre-merge-check "sudo -i -u slurm $MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます"
assert_not_contains "$(reason "$out")" "cd 先"

it "pre-merge-check: レビュー一覧を取得できなければ deny"
make_fake_gh_merge 'echo 0' 'echo "error connecting to api.github.com" >&2; exit 1'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "確認できません"

# ===========================================================================
# guard-branch-base: 既定ブランチ以外の上で、起点を省いてブランチを作らせない
# ===========================================================================
# 起点を省くと今の HEAD から作られ、作業中のブランチの未完了のコミット（別のトピック）が
# 新しいブランチに入る。既定ブランチの上なら HEAD = 既定ブランチなので止めない

REPO="$TEST_ROOT/branchbase"
make_repo "$REPO"
make_remote "$REPO"
git -C "$REPO" switch -q -c feat/wip
commit_file "$REPO" "wip.txt" "feat: wip"
git -C "$REPO" branch exist main
cd "$REPO" || exit 1

it "guard-branch-base: 起点のない git switch -c / -C は deny し、理由と修正方法を示す"
out=$(run_hook guard-branch-base 'git switch -c feat/other')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "feat/wip"
assert_contains "$(reason "$out")" "未完了のコミット"
assert_contains "$(reason "$out")" "worktree-add.sh feat/other"
assert_contains "$(reason "$out")" "git branch feat/other main"
assert_contains "$(reason "$out")" "git switch -c feat/other HEAD"
out=$(run_hook guard-branch-base 'git switch -C feat/other')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-branch-base 'git switch --create=feat/other')
assert_eq deny "$(decision "$out")"

it "guard-branch-base: 起点のない git checkout -b / -B は deny"
out=$(run_hook guard-branch-base 'git checkout -b feat/other')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git checkout -b feat/other HEAD"
out=$(run_hook guard-branch-base 'git checkout -B feat/other')
assert_eq deny "$(decision "$out")"

it "guard-branch-base: 起点のない git branch <b> は deny"
out=$(run_hook guard-branch-base 'git branch feat/other')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git branch feat/other HEAD"
out=$(run_hook guard-branch-base 'git branch --no-track feat/other')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-branch-base 'git branch -- feat/other')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-branch-base 'git branch -qf feat/other')
assert_eq deny "$(decision "$out")"

it "guard-branch-base: 起点のない git worktree add -b / -B は deny"
out=$(run_hook guard-branch-base 'git worktree add ../wt -b feat/other')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git worktree add ../wt -b feat/other HEAD"
out=$(run_hook guard-branch-base 'git worktree add -B feat/other ../wt')
assert_eq deny "$(decision "$out")"

it "guard-branch-base: git worktree add <path> がパスの名前で新しいブランチを作るときも deny"
out=$(run_hook guard-branch-base 'git worktree add ../newname')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "newname"

it "guard-branch-base: 起点を明示すれば（HEAD でも）許可する"
for cmd in 'git switch -c feat/other main' 'git switch -c feat/other HEAD' \
           'git checkout -b feat/other origin/main' 'git branch feat/other main' \
           'git branch -f feat/other HEAD' 'git worktree add ../wt -b feat/other main' \
           'git worktree add -b feat/other ../wt HEAD'; do
  out=$(run_hook guard-branch-base "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-branch-base: ブランチを作らない操作は対象外"
for cmd in 'git switch main' 'git switch -' 'git checkout main' 'git checkout -- wip.txt' \
           'git branch' 'git branch -a' 'git branch -d exist' 'git branch -D exist' \
           'git branch -m feat/renamed' 'git branch --show-current' 'git branch -vv' \
           'git branch --set-upstream-to=origin/main' 'git branch --list "feat/*"' \
           'git worktree add ../exist' 'git worktree add --detach ../det' \
           'git worktree list' 'git switch --orphan fresh' 'git checkout --orphan fresh'; do
  out=$(run_hook guard-branch-base "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-branch-base: 既定ブランチの上なら起点を省いても許可する"
git switch -q main
out=$(run_hook guard-branch-base 'git switch -c feat/other')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-branch-base 'git worktree add ../wt -b feat/other')
assert_eq allow "$(decision "$out")"

it "guard-branch-base: -C / cd で指定したリポジトリの今のブランチで判定する"
OTHER="$TEST_ROOT/branchbase-other"
make_repo "$OTHER"
git -C "$OTHER" switch -q -c feat/y
out=$(run_hook guard-branch-base "git -C $OTHER switch -c feat/other")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-branch-base "cd $OTHER && git switch -c feat/other")
assert_eq deny "$(decision "$out")"
git -C "$OTHER" switch -q main
git switch -q feat/wip
out=$(run_hook guard-branch-base "git -C $OTHER switch -c feat/other")
assert_eq allow "$(decision "$out")"

it "guard-branch-base: 同じコマンドの前の git switch で移った先のブランチで判定する"
git switch -q main
out=$(run_hook guard-branch-base 'git switch feat/wip && git switch -c feat/other')
assert_eq deny "$(decision "$out")"
git switch -q feat/wip
out=$(run_hook guard-branch-base 'git switch main && git switch -c feat/other')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-branch-base 'git checkout main && git checkout -b feat/other')
assert_eq allow "$(decision "$out")"

it "guard-branch-base: origin/HEAD があればそれを既定ブランチとする"
git push -q origin main:trunk
git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/trunk
out=$(run_hook guard-branch-base 'git switch -c feat/other')
assert_contains "$(reason "$out")" "git branch feat/other trunk"
git symbolic-ref --delete refs/remotes/origin/HEAD

LOCAL="$TEST_ROOT/branchbase-local"
make_repo "$LOCAL"
git -C "$LOCAL" switch -q -c feat/wip
cd "$LOCAL" || exit 1

it "guard-branch-base: リモートのないリポジトリでもローカルの main を既定ブランチとして止める"
out=$(run_hook guard-branch-base 'git switch -c feat/other')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git branch feat/other main"

it "guard-branch-base: 既定ブランチが見つからないリポジトリでは止めない（起点を示せない）"
NOMAIN="$TEST_ROOT/branchbase-nomain"
mkdir -p "$NOMAIN"
git -C "$NOMAIN" init -q -b work
commit_file "$NOMAIN" "a.txt" "chore: init"
cd "$NOMAIN" || exit 1
out=$(run_hook guard-branch-base 'git switch -c feat/other')
assert_eq allow "$(decision "$out")"

it "guard-branch-base: git リポジトリの外では何もしない"
cd "$TEST_ROOT" || exit 1
out=$(run_hook guard-branch-base 'git switch -c feat/other')
assert_eq allow "$(decision "$out")"

it "guard-branch-base: ヒアドキュメント本文のコマンドには反応しない"
cd "$REPO" || exit 1
out=$(run_hook guard-branch-base "$(write_doc '例: git switch -c feat/other')")
assert_eq allow "$(decision "$out")"

# ===========================================================================
# マージ前のトピック確認: 新しい ADR が 2 件以上のブランチは止める（1 ブランチ 1 トピック）
# ===========================================================================
# 決定は ADR に 1 件 1 決定で書くので、新しい ADR の数はブランチのトピックの数の目安になる。
# 既存の ADR の status の変化（superseded など）は数えない

TWO_ADRS='printf "added\t%s\n" docs/adr/0002-use-b.md docs/adr/0003-use-c.md src/x.rs'

it "pre-merge-check: 新しい ADR を 2 件以上加える PR は deny し、ADR を挙げて /topic-triage を示す"
cd "$TEST_ROOT/heredoc" || exit 1
make_fake_gh_merge 'echo 0' 'echo abc' "$TWO_ADRS"
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"
assert_contains "$(reason "$out")" "docs/adr/0003-use-c.md"
assert_contains "$(reason "$out")" "/topic-triage"
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC=<理由>"
# 「〜の場合は別」のような条件を文言に列挙しない（ADR-0023）
assert_not_contains "$(reason "$out")" "まとめて加える"
assert_not_contains "$(reason "$out")" "src/x.rs"

it "pre-merge-check: 新しい ADR が 1 件で、ほかは既存の ADR の変更・改名・README なら許可する"
make_fake_gh_merge 'echo 0' 'echo abc' 'printf "%s\t%s\n" added docs/adr/0002-use-b.md modified docs/adr/0001-keep-a.md renamed docs/adr/0004-new-name.md added docs/adr/README.md added docs/adr/drafts/0005-x.md added docs/adrs/0006-y.md'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: ALLOW_MULTI_TOPIC=<理由> なら新しい ADR が複数でもマージできる"
make_fake_gh_merge 'echo 0' 'echo abc' "$TWO_ADRS"
out=$(run_hook pre-merge-check "ALLOW_MULTI_TOPIC=テスト用 $MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: ALLOW_MULTI_TOPIC=1 は、本来の文言に理由を求める行を足して止める"
out=$(run_hook pre-merge-check "ALLOW_MULTI_TOPIC=1 $MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "/topic-triage"
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC には理由を記載"

it "pre-merge-check: PR の変更ファイルを取得できなければ deny"
make_fake_gh_merge 'echo 0' 'echo abc' 'echo "error connecting to api.github.com" >&2; exit 1'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "変更ファイル"

it "pre-merge-check: 変更ファイルを取得できないときも、短いエスケープには理由を求める行を足す"
out=$(run_hook pre-merge-check "ALLOW_MULTI_TOPIC=1 $MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "変更ファイル"
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC には理由を記載"

# --- ローカルの git merge（GitHub を使わないリポジトリは main に git merge --no-ff する）---

# write_adr <repo> <name> [status]
write_adr() {
  mkdir -p "$1/docs/adr"
  printf -- '---\nstatus: %s\n---\n\n# %s\n' "${3:-accepted}" "$2" > "$1/docs/adr/$2"
}

REPO="$TEST_ROOT/localmerge"
make_repo "$REPO"
write_adr "$REPO" 0001-keep-a.md
git -C "$REPO" add docs && git -C "$REPO" commit -q -m "docs: seed"
git -C "$REPO" switch -q -c feat/two
write_adr "$REPO" 0002-use-b.md proposed
write_adr "$REPO" 0003-use-c.md proposed
git -C "$REPO" add docs && git -C "$REPO" commit -q -m "docs(adr): b and c"
git -C "$REPO" switch -q -c feat/one main
write_adr "$REPO" 0004-use-d.md
write_adr "$REPO" 0001-keep-a.md superseded
echo "readme" > "$REPO/docs/adr/README.md"
git -C "$REPO" add docs && git -C "$REPO" commit -q -m "docs(adr): d supersedes a"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1

it "pre-git-merge-check: 既定ブランチに新しい ADR が 2 件以上のブランチをマージするなら deny"
out=$(run_hook pre-git-merge-check 'git merge --no-ff feat/two -m "Merge: two"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0002-use-b.md"
assert_contains "$(reason "$out")" "docs/adr/0003-use-c.md"
assert_contains "$(reason "$out")" "/topic-triage"
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC=<理由>"
out=$(run_hook pre-git-merge-check 'git merge -m "Merge: two" --no-ff feat/two')
assert_eq deny "$(decision "$out")"

it "pre-git-merge-check: 既存の ADR の status の変化と README は数えない"
out=$(run_hook pre-git-merge-check 'git merge --no-ff feat/one')
assert_eq allow "$(decision "$out")"
# 値を取るオプション（--cleanup）の値はマージ対象にしない
out=$(run_hook pre-git-merge-check 'git merge --no-ff --cleanup strip feat/one')
assert_eq allow "$(decision "$out")"

it "pre-git-merge-check: ALLOW_MULTI_TOPIC=<理由> で通す"
out=$(run_hook pre-git-merge-check 'ALLOW_MULTI_TOPIC=テスト用 git merge --no-ff feat/two')
assert_eq allow "$(decision "$out")"

it "pre-git-merge-check: git merge - は直前のブランチとして判定する"
git switch -q feat/two && git switch -q main
out=$(run_hook pre-git-merge-check 'git merge --no-ff -')
assert_eq deny "$(decision "$out")"

it "pre-git-merge-check: -C / cd で指定したリポジトリで判定する"
cd "$TEST_ROOT" || exit 1
out=$(run_hook pre-git-merge-check "git -C $REPO merge --no-ff feat/two")
assert_eq deny "$(decision "$out")"
out=$(run_hook pre-git-merge-check "cd $REPO && git merge --no-ff feat/two")
assert_eq deny "$(decision "$out")"
out=$(run_hook pre-git-merge-check "cd '$REPO/docs/adr' && git merge --no-ff feat/two")
assert_eq deny "$(decision "$out")"
cd "$REPO" || exit 1

it "pre-git-merge-check: 既定ブランチ以外へのマージ、マージの中断・再開は対象外"
git switch -q feat/one
out=$(run_hook pre-git-merge-check 'git merge feat/two')
assert_eq allow "$(decision "$out")"
git switch -q main
for cmd in 'git merge --abort' 'git merge --continue' 'git merge --quit' 'git merge-base main feat/two'; do
  out=$(run_hook pre-git-merge-check "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "pre-git-merge-check: リモート追跡ブランチの取り込み（main の更新）は対象外"
# origin/main には別々にマージされた複数のトピックが入っている
git -C "$REPO" init -q --bare "$REPO-origin.git"
git remote add origin "$REPO-origin.git"
git push -q origin feat/two:main
git fetch -q origin
out=$(run_hook pre-git-merge-check 'git merge --ff-only origin/main')
assert_eq allow "$(decision "$out")"

it "pre-git-merge-check: 日本語のファイル名の ADR も数える（git のパスのクォートに左右されない）"
git switch -q -c feat/ja main
write_adr "$REPO" 0005-日本語.md
write_adr "$REPO" 0006-別の決定.md
git add docs && git commit -q -m "docs(adr): ja"
git switch -q main
out=$(run_hook pre-git-merge-check 'git merge --no-ff feat/ja')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "docs/adr/0005-日本語.md"

it "pre-git-merge-check: 同じコマンドの前の git switch で移った先のブランチで判定する"
git switch -q feat/one
out=$(run_hook pre-git-merge-check 'git switch main && git merge --no-ff feat/two')
assert_eq deny "$(decision "$out")"
out=$(run_hook pre-git-merge-check 'git checkout main; git merge --no-ff feat/two')
assert_eq deny "$(decision "$out")"
git switch -q main
out=$(run_hook pre-git-merge-check 'git switch feat/one && git merge feat/two')
assert_eq allow "$(decision "$out")"
# 移った先が分からない（git switch -）なら、確かめる側に倒す
git switch -q feat/one
out=$(run_hook pre-git-merge-check 'git switch - && git merge --no-ff feat/two')
assert_eq deny "$(decision "$out")"
git switch -q main

it "pre-git-merge-check: diff.renames=false の設定でも ADR の改名は新しい ADR と数えない"
git switch -q -c feat/rename main
git mv docs/adr/0001-keep-a.md docs/adr/0001-keep-aa.md
write_adr "$REPO" 0007-use-g.md
git add docs && git commit -q -m "docs(adr): rename a, add g"
git switch -q main
git config diff.renames false
out=$(run_hook pre-git-merge-check 'git merge --no-ff feat/rename')
assert_eq allow "$(decision "$out")"
git config --unset diff.renames

it "pre-git-merge-check: マージ対象を標準入力で渡す --stdin は確かめられないので deny"
out=$(run_hook pre-git-merge-check "printf 'feat/two\\n' | git merge --no-ff --stdin")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--stdin"

it "pre-git-merge-check: docs/adr の無いリポジトリでは何もしない"
PLAIN="$TEST_ROOT/localmerge-plain"
make_repo "$PLAIN"
git -C "$PLAIN" switch -q -c feat/x
commit_file "$PLAIN" a.txt "feat: a"
commit_file "$PLAIN" b.txt "feat: b"
git -C "$PLAIN" switch -q main
cd "$PLAIN" || exit 1
out=$(run_hook pre-git-merge-check 'git merge --no-ff feat/x')
assert_eq allow "$(decision "$out")"

it "pre-git-merge-check: ヒアドキュメント本文のコマンドには反応しない"
cd "$REPO" || exit 1
out=$(run_hook pre-git-merge-check "$(write_doc '例: git merge --no-ff feat/two')")
assert_eq allow "$(decision "$out")"

# ===========================================================================
# guard-git-merge: 既定ブランチへのローカルのマージの形（--no-ff で、既定ブランチの上に rebase 済み）
# ===========================================================================

REPO="$TEST_ROOT/mergeshape"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/behind
commit_file "$REPO" behind.txt "feat: behind"
git -C "$REPO" switch -q main
commit_file "$REPO" main2.txt "feat: main moves on"
git -C "$REPO" switch -q -c feat/rebased
commit_file "$REPO" rebased.txt "feat: rebased"
git -C "$REPO" switch -q main
cd "$REPO" || exit 1

it "guard-git-merge: --no-ff で、既定ブランチの上に積まれたブランチなら通す"
out=$(run_hook guard-git-merge 'git merge --no-ff feat/rebased -m "Merge: rebased"')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-merge 'git merge -m "Merge: rebased" --no-ff feat/rebased')
assert_eq allow "$(decision "$out")"

it "guard-git-merge: --no-ff がなければ deny（fast-forward ではマージコミットが残らない）"
out=$(run_hook guard-git-merge 'git merge feat/rebased')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--no-ff"
out=$(run_hook guard-git-merge 'git merge --ff-only feat/rebased')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-merge 'git merge --squash feat/rebased')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-merge 'git merge --no-ff --ff feat/rebased')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-merge 'git merge --ff-only --no-ff feat/rebased')
assert_eq deny "$(decision "$out")"

it "guard-git-merge: 既定ブランチが先に進んでいて rebase していないブランチは deny"
out=$(run_hook guard-git-merge 'git merge --no-ff feat/behind -m "Merge: behind"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "feat/behind"
assert_contains "$(reason "$out")" "git rebase main"

it "guard-git-merge: git merge - は直前のブランチとして判定する"
git switch -q feat/behind && git switch -q main
out=$(run_hook guard-git-merge 'git merge --no-ff -')
assert_eq deny "$(decision "$out")"

it "guard-git-merge: -C / cd で指定したリポジトリで判定する"
cd "$TEST_ROOT" || exit 1
out=$(run_hook guard-git-merge "git -C $REPO merge feat/rebased")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-merge "cd $REPO && git merge --no-ff feat/behind")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-merge "git -C $REPO merge --no-ff feat/rebased -m 'Merge: rebased'")
assert_eq allow "$(decision "$out")"
cd "$REPO" || exit 1

it "guard-git-merge: 既定ブランチ以外へのマージ、中断・再開、リモート追跡ブランチの取り込みは対象外"
git switch -q feat/rebased
out=$(run_hook guard-git-merge 'git merge feat/behind')
assert_eq allow "$(decision "$out")"
git switch -q main
for cmd in 'git merge --abort' 'git merge --continue' 'git merge --quit' 'git merge-base main feat/behind'; do
  out=$(run_hook guard-git-merge "$cmd")
  assert_eq allow "$(decision "$out")"
done
git init -q --bare "$REPO-origin.git"
git remote add origin "$REPO-origin.git"
git push -q origin main
git fetch -q origin
out=$(run_hook guard-git-merge 'git merge --ff-only origin/main')
assert_eq allow "$(decision "$out")"

it "guard-git-merge: 同じコマンドの前の git switch で移った先のブランチで判定する"
git switch -q feat/rebased
out=$(run_hook guard-git-merge 'git switch main && git merge feat/rebased')
assert_eq deny "$(decision "$out")"

it "guard-git-merge: 前の git switch は、サブシェルや bash -c の文字列の中のコマンドにも効く"
for cmd in "git switch main && bash -c 'git merge feat/rebased'" 'git switch main; (git merge feat/rebased)' \
  'git switch main && eval git merge feat/rebased'; do
  out=$(run_hook guard-git-merge "$cmd")
  assert_eq deny "$(decision "$out")"
done
# サブシェルや文字列の中の switch は外に漏れない（feat/rebased のまま、既定ブランチ以外へのマージは対象外）
for cmd in "bash -c 'git switch main'; git merge feat/behind" '(git switch main); git merge feat/behind'; do
  out=$(run_hook guard-git-merge "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-git-merge: 対象のブランチの worktree に未追跡のファイルがあれば deny（後片付けの worktree remove が止まる）"
cd "$REPO" || exit 1
git switch -q main
git worktree add -q -b feat/wt "$TEST_ROOT/mergeshape-wt" main
commit_file "$TEST_ROOT/mergeshape-wt" wt.txt "feat: wt"
out=$(run_hook guard-git-merge 'git merge --no-ff feat/wt -m "Merge: wt"')
assert_eq allow "$(decision "$out")"
echo out >"$TEST_ROOT/mergeshape-wt/trial1.out"
echo out >"$TEST_ROOT/mergeshape-wt/trial2.out"
out=$(run_hook guard-git-merge 'git merge --no-ff feat/wt -m "Merge: wt"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "（2 件）"
assert_contains "$(reason "$out")" "$TEST_ROOT/mergeshape-wt"
assert_contains "$(reason "$out")" "trial1.out"
assert_contains "$(reason "$out")" "コミット"
assert_contains "$(reason "$out")" ".gitignore"

it "guard-git-merge: 未コミットの変更が多いときは先頭の数件と残りの件数を示す"
for i in 3 4 5 6 7 8 9; do echo out >"$TEST_ROOT/mergeshape-wt/trial$i.out"; done
out=$(run_hook guard-git-merge 'git merge --no-ff feat/wt -m "Merge: wt"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "（9 件）"
assert_contains "$(reason "$out")" "ほか 4 件"
rm "$TEST_ROOT"/mergeshape-wt/trial*.out

it "guard-git-merge: 対象のブランチの worktree に未コミットの変更があれば deny"
echo changed >"$TEST_ROOT/mergeshape-wt/wt.txt"
out=$(run_hook guard-git-merge 'git merge --no-ff feat/wt -m "Merge: wt"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "wt.txt"
git -C "$TEST_ROOT/mergeshape-wt" checkout -q -- wt.txt

it "guard-git-merge: 無視されたファイルだけの worktree は通す"
echo '*.cache' >"$TEST_ROOT/mergeshape-wt/.gitignore"
git -C "$TEST_ROOT/mergeshape-wt" add .gitignore
git -C "$TEST_ROOT/mergeshape-wt" commit -q -m "chore: ignore cache"
echo x >"$TEST_ROOT/mergeshape-wt/a.cache"
out=$(run_hook guard-git-merge 'git merge --no-ff feat/wt -m "Merge: wt"')
assert_eq allow "$(decision "$out")"

it "guard-git-merge: 既定ブランチの worktree の未追跡のファイルは検査しない（git が自分で止める）"
echo x >"$REPO/untracked-on-main.txt"
out=$(run_hook guard-git-merge 'git merge --no-ff feat/wt -m "Merge: wt"')
assert_eq allow "$(decision "$out")"
rm "$REPO/untracked-on-main.txt"
git worktree remove --force "$TEST_ROOT/mergeshape-wt"
git branch -q -D feat/wt

it "guard-sbatch: sbatch を直接投げたら slurm-run.sh を示して止める"
out=$(run_hook guard-sbatch "sbatch --exclusive --wait --wrap 'true'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "slurm-run.sh"

it "guard-sbatch: command -v / -pv / -V はプログラムを探すだけで、実行しない"
for cmd in 'command -v sbatch' 'command -pv sbatch' 'command -Vp sbatch'; do
  out=$(run_hook guard-sbatch "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-sbatch: パイプやつなぎの中の sbatch も止める"
out=$(run_hook guard-sbatch "cd ~/github/xlc && sbatch --wrap 'true' | tail -1")
assert_eq deny "$(decision "$out")"

it "guard-sbatch: slurm-run.sh をフォアグラウンドで実行したら止める"
out=$(run_hook guard-sbatch "$HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "run_in_background"

it "guard-sbatch: slurm-run.sh を background で実行するのは通す"
out=$(run_hook guard-sbatch "$HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'" true)
assert_eq allow "$(decision "$out")"

it "guard-sbatch: slurm-run.sh は、同名のテスト以外ならどの書き方でもフォアグラウンドなら止める"
# shellcheck disable=SC2088  # 展開前のコマンド文字列をそのまま hook に渡す
for invoked in ".claude/scripts/slurm-run.sh" "./.claude/scripts/slurm-run.sh" "~/.claude/scripts/slurm-run.sh" \
  "$HOME/.claude/scripts/slurm-run.sh" "\"$HOME/.claude/scripts/slurm-run.sh\"" \
  "~/.claude/scripts/./slurm-run.sh" "~/.claude/scripts//slurm-run.sh" "scripts/slurm-run.sh" \
  "../scripts/slurm-run.sh" "/nix/store/abc-claude-scripts/slurm-run.sh" \
  "/home/tagawa/nix/nixfiles-feat-x/.claude/scripts/slurm-run.sh" "bash .claude/scripts/slurm-run.sh" \
  "env FOO=1 ~/.claude/scripts/slurm-run.sh" "slurm-run.sh" "./slurm-run.sh" "bash slurm-run.sh" \
  "foo.claude/tests/scripts/slurm-run.sh" "/tmp/x/tests/scripts/slurm-run.sh"; do
  out=$(run_hook guard-sbatch "$invoked -t 5 xlc-1B 'true'")
  assert_eq deny "$(decision "$out")"
done

it "guard-sbatch: 同名のテスト（.claude/tests/scripts/slurm-run.sh）は Slurm に投げないので通す"
for invoked in "bash .claude/tests/scripts/slurm-run.sh" "bash ./.claude/tests/scripts/slurm-run.sh" \
  ".claude/tests/scripts/slurm-run.sh" "bash ~/nix/nixfiles/.claude/tests/scripts/slurm-run.sh" \
  "bash /home/tagawa/nix/nixfiles-feat-x/.claude/tests/scripts/slurm-run.sh"; do
  out=$(run_hook guard-sbatch "$invoked")
  assert_eq allow "$(decision "$out")"
done

it "guard-sbatch: command、env、exec、nohup、time で包んだ sbatch も止める"
for wrapped in "command sbatch --wrap true" "env FOO=1 sbatch --wrap true" "env -i sbatch --wrap true" \
  "exec sbatch --wrap true" "nohup sbatch --wrap true" "time sbatch --wrap true" "command -p env sbatch --wrap true" \
  "env -u FOO sbatch --wrap true" "env -C /tmp sbatch --wrap true" "env --unset=FOO sbatch --wrap true" \
  "exec -a alias sbatch --wrap true" "time -o t.txt sbatch --wrap true" "time -f %e sbatch --wrap true" \
  "env -a alias sbatch --wrap true" "env --argv0 alias sbatch --wrap true" "env -S 'sbatch --wrap true'" "env --split-string='sbatch --wrap true'" "env -S'sbatch --wrap true'"; do
  out=$(run_hook guard-sbatch "$wrapped")
  assert_eq deny "$(decision "$out")"
done

it "guard-sbatch: 包んだ slurm-run.sh もフォアグラウンドなら止める"
out=$(run_hook guard-sbatch "command $HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-sbatch "nohup bash $HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-sbatch "env -u FOO $HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
assert_eq deny "$(decision "$out")"

it "guard-sbatch: bash / sh のオプションを読み飛ばし、その後ろの語をスクリプトとして見る（#303）"
for invoked in "bash -x" "bash -e" "sh -eu" "bash -o pipefail" "bash --rcfile ~/.bashrc" "bash --" "bash +O extglob" \
  "nohup bash -x" "bash -xeu -o errexit --" "bash -"; do
  out=$(run_hook guard-sbatch "$invoked $HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
  assert_eq deny "$(decision "$out")"
  out=$(run_hook guard-sbatch "$invoked .claude/tests/scripts/slurm-run.sh")
  assert_eq allow "$(decision "$out")"
done
# -n は構文を調べるだけで実行しない
out=$(run_hook guard-sbatch "bash -n $HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
assert_eq allow "$(decision "$out")"

it "guard-sbatch: bash -c / sh -c の文字列の中も、深さ 5 まで再帰的に読む"
for cmd in "bash -c 'sbatch --wrap true'" "sh -c \"sbatch --wrap true\"" "bash -ec 'sbatch --wrap true'" \
  "bash -o pipefail -c 'cd /tmp && sbatch --wrap true' arg0 arg1" "bash -c \"bash -c 'sbatch --wrap true'\"" \
  "env FOO=1 bash -c 'sbatch --wrap true'" "nohup sh -c 'sleep 1; sbatch --wrap true' &" \
  "bash -c 'bash -c \"bash -c \\\"sbatch --wrap true\\\"\"'"; do
  out=$(run_hook guard-sbatch "$cmd")
  assert_eq deny "$(decision "$out")"
done
out=$(run_hook guard-sbatch "bash -c '$HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B true'")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-sbatch "bash -c '$HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B true'" true)
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-sbatch "bash -c 'echo sbatch; squeue'")
assert_eq allow "$(decision "$out")"
# 文字列以外の引数（$0 以降）の中の置換は、外側のシェルが bash を起動する前に実行する
out=$(run_hook guard-sbatch "bash -c 'true' \"\$(sbatch --wrap true)\"")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-sbatch "bash -c \"squeue; \$(sbatch --wrap true)\"")
assert_eq deny "$(decision "$out")"
# 深さの上限（5）を超える入れ子は、読めない形として、対象語があれば止める側に倒す
deep="sbatch --wrap true"
for _ in 1 2 3 4 5 6; do deep="bash -c $(printf '%q' "$deep")"; done
out=$(run_hook guard-sbatch "$deep")
assert_eq deny "$(decision "$out")"
deep="squeue"
for _ in 1 2 3 4 5 6; do deep="bash -c $(printf '%q' "$deep")"; done
out=$(run_hook guard-sbatch "$deep")
assert_eq allow "$(decision "$out")"

it "guard-sbatch: sudo、xargs、flock、chrt、taskset、watch、script -c、ssh、eval で包んだ sbatch も止める"
for wrapped in "sudo sbatch --wrap true" "sudo -u slurm -g users sbatch --wrap true" "sudo -E FOO=1 sbatch --wrap true" \
  "sudo --user=slurm -n -- sbatch --wrap true" "sudo -H -D /tmp sbatch --wrap true" \
  "echo x | xargs sbatch --wrap" "xargs -0 -I{} sbatch --wrap {}" "xargs -n1 -P 4 -d '\n' sbatch --wrap" \
  "xargs -r --max-args=1 sbatch --wrap" "xargs -I {} -t sbatch --wrap {}" \
  "flock /tmp/lock sbatch --wrap true" "flock -w 10 -x /tmp/lock sbatch --wrap true" \
  "flock --timeout=10 /tmp/lock -c 'sbatch --wrap true'" "flock -n /tmp/lock -c 'cd /tmp && sbatch --wrap true'" \
  "chrt -f 10 sbatch --wrap true" "chrt -i 0 sbatch --wrap true" "chrt --idle 0 sbatch --wrap true" \
  "chrt -T 1000 -d 0 sbatch --wrap true" "taskset 0x1 sbatch --wrap true" "taskset -c 0-3 sbatch --wrap true" \
  "taskset --cpu-list 0 sbatch --wrap true" "watch sbatch --wrap true" "watch -n 5 'sbatch --wrap true'" \
  "watch -x sbatch --wrap true" "watch --interval=5 -d sbatch --wrap true" "watch -dpermanent 'squeue; sbatch --wrap true'" \
  "script -c 'sbatch --wrap true' log.txt" "script -q --command 'sbatch --wrap true'" "script log.txt -- sbatch --wrap true" \
  "ssh r995 sbatch --wrap true" "ssh -p 22 -o StrictHostKeyChecking=no user@r995 'sbatch --wrap true'" \
  "ssh -t r995 'cd ~/x && sbatch --wrap true'" "ssh -i ~/.ssh/id -J bastion r995 sbatch --wrap true" \
  "ssh r995 -t sbatch --wrap true" "ssh r995 -p 22 -- sbatch --wrap true" "script log.txt -c 'sbatch --wrap true'" \
  "script log.txt -q --command 'sbatch --wrap true'" \
  "eval 'sbatch --wrap true'" "eval sbatch --wrap true" "eval \"cd /tmp; sbatch --wrap true\"" \
  "sudo flock /tmp/lock chrt -f 10 sbatch --wrap true" "ssh r995 bash -c 'sbatch --wrap true'"; do
  out=$(run_hook guard-sbatch "$wrapped")
  assert_eq deny "$(decision "$out")"
done

it "guard-sbatch: ラッパーが実行しない形（sudo -l / -e / -v、chrt -p、taskset -p、ssh -N / -G、flock <fd>）は通す"
for cmd in "sudo -l sbatch" "sudo -e sbatch" "sudo -v" "sudo -K" "chrt -p 10 1234" "chrt -m" "taskset -p 1 1234" \
  "taskset -ap 1234" "ssh -N r995" "ssh r995 -N" "ssh -G r995" "ssh -Q cipher" "flock 9" "flock -u 9" "script log.txt" \
  "env FOO=1 -i sbatch --wrap true" "bash --help sbatch --wrap true" "bash --version sbatch" "xargs --help sbatch" \
  "env --version sbatch" "watch --help sbatch" "sudo --help sbatch"; do
  out=$(run_hook guard-sbatch "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-sbatch: 実行するプログラムを決められない形は、コマンド文字列に対象語があれば止める側に倒す"
for cmd in 'CMD=sbatch; $CMD --wrap true' 'CMD="sbatch --wrap true"; eval "$CMD"' '$(which sbatch) --wrap true' \
  '. ./run-sbatch.sh' 'source run-sbatch.sh' 'bash -s < run-sbatch.sh' 'bash < run-sbatch.sh' \
  'echo "sbatch --wrap true" | sh' 'bash -c "$CMD" # sbatch' "bash -c \"\$SBATCH_BIN --wrap true\"; echo sbatch" \
  'ssh r995 < run-sbatch.sh' 'bash "$SCRIPT" sbatch' 'sudo -s < run-sbatch.sh' 'eval "$CMD" sbatch' \
  '${SB:-/usr/bin/sbatch} --wrap true' '$(command -v /usr/bin/sbatch) --wrap true'; do
  out=$(run_hook guard-sbatch "$cmd")
  assert_eq deny "$(decision "$out")"
done
for cmd in '$CMD --wrap true' 'eval "$CMD"' 'source venv/bin/activate && squeue' '. ~/.bashrc' 'bash -s < run.sh' \
  'bash "$SCRIPT" -t 5' 'ssh r995' 'eval "$(ssh-agent)"'; do
  out=$(run_hook guard-sbatch "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-sbatch: ALLOW_RAW_SBATCH=<理由> を付ければ sbatch を直接投げられる"
out=$(run_hook guard-sbatch "ALLOW_RAW_SBATCH=テスト用 sbatch --array=1-4 --wrap 'true'")
assert_eq allow "$(decision "$out")"

it "guard-sbatch: 止めるときにエスケープの名前を示す"
out=$(run_hook guard-sbatch "sbatch --wrap 'true'")
assert_contains "$(reason "$out")" "ALLOW_RAW_SBATCH=<理由>"
out=$(run_hook guard-sbatch "$HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
assert_contains "$(reason "$out")" "ALLOW_FOREGROUND_SLURM=<理由>"

it "guard-sbatch: ALLOW_FOREGROUND_SLURM=<理由> を付ければ slurm-run.sh をフォアグラウンドで実行できる"
out=$(run_hook guard-sbatch "ALLOW_FOREGROUND_SLURM=テスト用 $HOME/.claude/scripts/slurm-run.sh -t 1 xlc-1B 'true'")
assert_eq allow "$(decision "$out")"

it "guard-sbatch: エスケープはそれぞれの検査だけを外す"
out=$(run_hook guard-sbatch "ALLOW_FOREGROUND_SLURM=テスト用 sbatch --wrap 'true'")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-sbatch "ALLOW_RAW_SBATCH=テスト用 $HOME/.claude/scripts/slurm-run.sh -t 5 xlc-1B 'true'")
assert_eq deny "$(decision "$out")"

it "guard-sbatch: 理由の無いエスケープは、理由を求めて止める"
out=$(run_hook guard-sbatch "ALLOW_RAW_SBATCH=1 sbatch --wrap 'true'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ALLOW_RAW_SBATCH には理由を記載"

it "guard-sbatch: command -v と -V は探すだけなので通す"
out=$(run_hook guard-sbatch "command -v sbatch && command -V sbatch")
assert_eq allow "$(decision "$out")"

it "guard-sbatch: squeue、scontrol、引数に現れるだけの sbatch は通す"
out=$(run_hook guard-sbatch "squeue; scontrol top 42; echo sbatch; grep -n sbatch CLAUDE.md")
assert_eq allow "$(decision "$out")"

it "heredoc: 本文中の sbatch はコマンドとみなさない（guard-sbatch）"
out=$(run_hook guard-sbatch "$(write_doc '重い計算は sbatch でなく slurm-run.sh で投げる')")
assert_eq allow "$(decision "$out")"

# ===========================================================================
# require-background-wait: ラッパーと bash の文字列の中の待機スクリプトも見る
# ===========================================================================

it "require-background-wait: bash のオプションつき、bash -c の文字列、ラッパーの中の待機スクリプトも止める"
PR12=https://github.com/o/r/pull/12
for cmd in "bash -x $HOME/.claude/scripts/gh-wait-review.sh $PR12" "sh -eu ~/.claude/scripts/request-rereview.sh $PR12" \
  "bash -c '$HOME/.claude/scripts/gh-wait-review.sh $PR12'" "sudo ~/.claude/scripts/gh-wait-review.sh $PR12" \
  "eval ~/.claude/scripts/gh-wait-review.sh $PR12" "flock /tmp/l ~/.claude/scripts/gh-wait-review.sh $PR12" \
  "ssh r995 ~/.claude/scripts/gh-wait-review.sh $PR12"; do
  out=$(run_hook require-background-wait "$cmd")
  assert_eq deny "$(decision "$out")"
  out=$(run_hook require-background-wait "$cmd" true)
  assert_eq allow "$(decision "$out")"
done

it "require-background-wait: 実行するプログラムを決められない形は、文字列に対象のスクリプト名があれば止める"
out=$(run_hook require-background-wait 'WAIT=~/.claude/scripts/gh-wait-review.sh; $WAIT https://github.com/o/r/pull/12')
assert_eq deny "$(decision "$out")"
out=$(run_hook require-background-wait 'WAIT=~/.claude/scripts/gh-wait-review.sh; $WAIT https://github.com/o/r/pull/12' true)
assert_eq allow "$(decision "$out")"
out=$(run_hook require-background-wait '$WAIT 12')
assert_eq allow "$(decision "$out")"

# ===========================================================================
# guard-git-stash: git stash はリポジトリの全 worktree と全セッションで共有なので止める
# ===========================================================================
# 読むだけの list / show は通す。退避は WIP のコミットか、テストの部分だけのステージで行う

it "guard-git-stash: stash の書き込み系は deny し、代わりの手順を示す"
for cmd in 'git stash' 'git stash push' 'git stash push -m wip' 'git stash -u' 'git stash save wip' 'git stash pop' \
  'git stash apply' 'git stash drop' 'git stash clear' 'git stash branch feat/x' 'git stash create' 'git stash store abc' \
  'git -C /tmp/repo stash' 'git -c core.editor=true stash pop' 'git stash -- file.txt' \
  'env FOO=1 git stash' 'sudo git stash' "bash -c 'git stash pop'" 'git add a && git stash' \
  'git --no-pager stash' 'command git stash' 'xargs git stash' 'CMD="git stash pop"; $CMD' 'eval "git stash pop"'; do
  out=$(run_hook guard-git-stash "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "コミット"
  assert_contains "$(reason "$out")" "共有"
done

it "guard-git-stash: list と show は通す"
for cmd in 'git stash list' 'git stash show' 'git stash show -p stash@{0}' 'git -C /tmp/repo stash list'; do
  out=$(run_hook guard-git-stash "$cmd")
  assert_eq "" "$out"
done

it "guard-git-stash: stash 以外と、文字列の中の git stash は対象外"
for cmd in 'git status' 'git commit -m "docs: do not git stash"' 'git log --grep stash' 'echo git stash'; do
  out=$(run_hook guard-git-stash "$cmd")
  assert_eq "" "$out"
done

# ===========================================================================
# guard-git-reset-hard: 未コミットの変更がある worktree での git reset --hard を止める
# ===========================================================================
# 対象のディレクトリと、-C が無ければ hook の cwd（前の cd が失敗すると ; の後の reset はそこで走る）を見る

RESET_DIRTY="$TEST_ROOT/reset-dirty"
RESET_CLEAN="$TEST_ROOT/reset-clean"
make_repo "$RESET_DIRTY"
make_repo "$RESET_CLEAN"
echo edited >"$RESET_DIRTY/README.md"

it "guard-git-reset-hard: 対象の worktree に未コミットの変更があれば deny し、変更を挙げる"
cd "$RESET_CLEAN" || exit 1
for cmd in "cd $RESET_DIRTY && git reset --hard" "git -C $RESET_DIRTY reset --hard HEAD" \
  "cd $RESET_DIRTY; git reset -q --hard origin/main"; do
  out=$(run_hook guard-git-reset-hard "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "$RESET_DIRTY"
  assert_contains "$(reason "$out")" "README.md"
done

it "guard-git-reset-hard: cd が失敗したときに走る hook の cwd に変更があれば deny し、-C を示す"
cd "$RESET_DIRTY" || exit 1
out=$(run_hook guard-git-reset-hard "cd $TEST_ROOT/no-such-dir && git rm -q x; git reset -q --hard")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "$RESET_DIRTY"
assert_contains "$(reason "$out")" "git -C <絶対パス> reset --hard"
out=$(run_hook guard-git-reset-hard "cd $RESET_CLEAN && git reset --hard")
assert_eq deny "$(decision "$out")"

it "guard-git-reset-hard: -C で変更の無い worktree を指せば、hook の cwd の変更に依らず通す"
cd "$RESET_DIRTY" || exit 1
out=$(run_hook guard-git-reset-hard "git -C $RESET_CLEAN reset --hard")
assert_eq "" "$out"

it "guard-git-reset-hard: 変更が無い、追跡していないファイルだけ、--hard でない reset は通す"
cd "$RESET_CLEAN" || exit 1
echo new >"$RESET_CLEAN/untracked.txt"
out=$(run_hook guard-git-reset-hard "git reset --hard")
assert_eq "" "$out"
rm "$RESET_CLEAN/untracked.txt"
cd "$RESET_DIRTY" || exit 1
for cmd in 'git reset' 'git reset --soft HEAD~1' 'git reset --keep HEAD' 'git status' 'echo git reset --hard'; do
  out=$(run_hook guard-git-reset-hard "$cmd")
  assert_eq "" "$out"
done

it "guard-git-reset-hard: まだ無いディレクトリへの cd の後の reset は、そのディレクトリを検査しない"
cd "$RESET_CLEAN" || exit 1
out=$(run_hook guard-git-reset-hard "mkdir -p $TEST_ROOT/later && cd $TEST_ROOT/later && git init -q && git reset --hard")
assert_eq "" "$out"

it "guard-git-reset-hard: 前の cd が通った後の cd の失敗、bash -c の中、--hard の省略形、alias も見る"
cd "$RESET_CLEAN" || exit 1
for cmd in "cd $RESET_DIRTY && cd $TEST_ROOT/no-such-dir; git reset --hard" \
  "cd $RESET_DIRTY && bash -c 'cd $TEST_ROOT/no-such-dir; git reset --hard'" \
  "git -C $RESET_DIRTY reset --har" "git -C $RESET_DIRTY reset --ha HEAD" \
  "git -C $RESET_DIRTY -c alias.rh='reset --hard' rh"; do
  out=$(run_hook guard-git-reset-hard "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "$RESET_DIRTY"
done

it "guard-git-reset-hard: 対象のディレクトリを書き方から決められなければ deny し、絶対パスの -C を示す"
cd "$RESET_CLEAN" || exit 1
for cmd in "d=$RESET_DIRTY; git -C \$d reset --hard" "d=$RESET_DIRTY; cd \$d && git reset --hard" \
  "git --git-dir=$RESET_DIRTY/.git --work-tree=$RESET_DIRTY reset --hard" \
  "GIT_DIR=$RESET_DIRTY/.git GIT_WORK_TREE=$RESET_DIRTY git reset --hard" \
  "x='git reset --hard'; cd $RESET_DIRTY && \$x"; do
  out=$(run_hook guard-git-reset-hard "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "決められません"
done

it "guard-git-reset-hard: 絶対パスの env -C で指したもの、ssh の先は、hook の cwd の変更に依らず通す"
cd "$RESET_DIRTY" || exit 1
for cmd in "env -C $RESET_CLEAN git reset --hard" "ssh host 'cd repo && git reset --hard'"; do
  out=$(run_hook guard-git-reset-hard "$cmd")
  assert_eq "" "$out"
done

it "guard-git-reset-hard: 決められない形でも、--hard の無いコマンドと、絶対パスで指した reset は止めない"
cd "$RESET_DIRTY" || exit 1
for cmd in 'source ~/.bashrc; git reset --soft HEAD~1' '$EDITOR notes.txt; echo preset' \
  "cd \$HOME && ls; git -C $RESET_CLEAN reset --hard" "git -C ../reset-clean reset --hard" \
  "env -C ../reset-clean git reset --hard" "env -C $RESET_CLEAN bash -c 'git reset --hard'"; do
  out=$(run_hook guard-git-reset-hard "$cmd")
  assert_eq "" "$out"
done

it "guard-git-reset-hard: 包んだ絶対パスの env -C の中の cd は、失敗しても hook の cwd に落ちないので cwd を見ない"
cd "$RESET_DIRTY" || exit 1
mkdir -p "$RESET_CLEAN/sub"
out=$(run_hook guard-git-reset-hard "env -C $RESET_CLEAN bash -c 'cd sub; git reset --hard'")
assert_eq "" "$out"
cd "$RESET_CLEAN" || exit 1
out=$(run_hook guard-git-reset-hard "env -C $RESET_DIRTY bash -c 'cd $TEST_ROOT/no-such-dir; git reset --hard'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "$RESET_DIRTY"

it "guard-git-reset-hard: export した GIT_DIR、pushd の後は、ディレクトリを決められないとして deny"
cd "$RESET_CLEAN" || exit 1
for cmd in "export GIT_DIR=$RESET_DIRTY/.git GIT_WORK_TREE=$RESET_DIRTY; git reset --hard" \
  "pushd $RESET_DIRTY && git reset --hard"; do
  out=$(run_hook guard-git-reset-hard "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "決められません"
done

it "guard-git-reset-hard: サブモジュールの変更も挙げ、変更の一覧は 5 件までにする"
RESET_SUPER="$TEST_ROOT/reset-super"
make_repo "$RESET_SUPER"
git -C "$RESET_SUPER" -c protocol.file.allow=always submodule -q add "$RESET_CLEAN" sub
git -C "$RESET_SUPER" commit -q -m "chore: add sub"
echo edited >"$RESET_SUPER/sub/README.md"
out=$(run_hook guard-git-reset-hard "git -C $RESET_SUPER -c submodule.recurse=true reset --hard")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "sub"
RESET_MANY="$TEST_ROOT/reset-many"
make_repo "$RESET_MANY"
for i in 1 2 3 4 5 6 7; do commit_file "$RESET_MANY" "f$i.txt" "chore: f$i"; echo x >>"$RESET_MANY/f$i.txt"; done
out=$(run_hook guard-git-reset-hard "git -C $RESET_MANY reset --hard")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "ほか 2 件"

it "guard-git-reset-hard: worktree の中で git status が失敗すれば deny"
RESET_BROKEN="$TEST_ROOT/reset-broken"
make_repo "$RESET_BROKEN"
echo garbage >"$RESET_BROKEN/.git/index"
out=$(run_hook guard-git-reset-hard "git -C $RESET_BROKEN reset --hard")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git status が失敗"

it "guard-git-reset-hard: ALLOW_GIT_RESET_HARD=<理由> で迂回できる"
cd "$RESET_DIRTY" || exit 1
out=$(run_hook guard-git-reset-hard 'ALLOW_GIT_RESET_HARD=テスト用 git reset --hard')
assert_eq "" "$out"

# ===========================================================================
# guard-exclusive-slurm: 排他の Slurm のジョブが実行中は、全コアを使う手元のコマンドを止める
# ===========================================================================
# 排他のジョブの時間の記録を乱すため（ADR-0041）。squeue は偽のコマンドで置き換える

FAKE_SQUEUE="$TEST_ROOT/fake-squeue"
mkdir -p "$FAKE_SQUEUE/exclusive" "$FAKE_SQUEUE/shared" "$FAKE_SQUEUE/idle"
printf '#!/bin/sh\nprintf "NO|openmc-2K|1234\\nOK|x-shared|1235\\n"\n' >"$FAKE_SQUEUE/exclusive/squeue"
printf '#!/bin/sh\nprintf "OK|x-shared|1235\\n"\n' >"$FAKE_SQUEUE/shared/squeue"
printf '#!/bin/sh\nexit 0\n' >"$FAKE_SQUEUE/idle/squeue"
mkdir -p "$FAKE_SQUEUE/many"
printf '#!/bin/sh\nseq 1 30000 | while read -r n; do echo "YES|shared-$n|$n"; done\necho "NO|openmc-2K|99999"\n' >"$FAKE_SQUEUE/many/squeue"
chmod +x "$FAKE_SQUEUE"/*/squeue
cd "$TEST_ROOT" || exit 1

it "guard-exclusive-slurm: 排他のジョブ中の全コアのコマンドは deny し、ジョブ名と番号と修正方法を示す"
for cmd in 'cargo build' 'cargo test --release' 'cargo clippy --all-targets' 'cargo run -- x' 'cargo bench' 'cargo check' 'nix flake check' \
  'nix develop -c cargo test' 'nix develop ../..#references -c dvc repro' 'nix develop --command bin/xs in.toml' \
  'cargo test -j 1' 'cargo run --jobs=1 -- x' 'cargo bench -j1' 'cargo build -j 1' 'cargo build --jobs=1' 'cargo b -j1' 'cargo check -j 1' 'cargo clippy -j1' 'cargo c --jobs 1' 'cargo -j 1 check' \
  'CARGO_BUILD_JOBS=1 cargo check' \
  'CMD=cargo; $CMD test' 'bash -s <<< "cargo test"' 'source x.sh' 'bash "$SCRIPT"' 'eval "$CMD"' \
  'nix --arg x 1 build .#x' 'nix --foo bar build .#x' 'cargo --weird test' 'dvc --cd sub repro' 'nix develop --option k v -c cargo test' \
  'nix develop --expr 1 -c cargo test' 'target/x86_64-unknown-linux-gnu/release/xs in.toml' '/tmp/tgt/debug/lattice in.toml' \
  'cargo b' 'cargo c' 'cargo t' 'cargo r -- x' 'cargo --color never test' 'cargo -C /tmp build' \
  'nix -j 4 build .#x' 'nix --max-jobs 2 build .#x' 'nix --cores 2 build .#x' 'nix -I nixpkgs=. build .#x' 'nix --store /tmp/s build .#x' \
  'nix --log-format bar build .#x' 'nix -L develop -c cargo test' \
  "nix develop ../..#references -c bash -c 'cargo test'" "nix develop -c bash -c 'taskset -c 0-31 cargo test'" \
  'nix shell nixpkgs#hello -c cargo test' 'nix shell -c dvc repro' \
  "taskset -c 3 bash -c 'taskset -c 0-31 cargo test'" 'taskset -c 3 nix develop -c taskset -c 0-31 cargo test' \
  'target/release/xs in.toml' './target/debug/lattice in.toml' '../../target/release/core in.toml' \
  'CARGO_BUILD_JOBS=2 cargo check' 'CARGO_BUILD_JOBS=1 cargo test' \
  'cargo +nightly build' 'cargo build -j 4' 'dvc repro' 'dvc repro examples/vera/dvc.yaml' \
  'bin/xs in.toml' './bin/lattice in.toml' '../../bin/core in.toml' '../bin/chain in.toml' 'nix build .#x' \
  'cargo --config a=b test' 'nix --extra-experimental-features nix-command build .#x' 'taskset -c 3 taskset -c 0-31 cargo test' 'cd repo && cargo test' 'env FOO=1 cargo test' "bash -c 'cargo test'" 'taskset -c 0-3 cargo test' 'taskset 0x3 cargo build'; do
  out=$(PATH="$FAKE_SQUEUE/exclusive:$PATH" run_hook guard-exclusive-slurm "$cmd")
  assert_eq deny "$(decision "$out")"
  assert_contains "$(reason "$out")" "openmc-2K"
  assert_contains "$(reason "$out")" "1234"
  assert_contains "$(reason "$out")" "taskset"
  assert_contains "$(reason "$out")" "slurm-run.sh"
done

it "guard-exclusive-slurm: -j 1 か 1 コアの taskset で限れば通す"
for cmd in 'taskset -c 3 cargo check' 'taskset -c 3 cargo clippy --all-targets' 'taskset -c 3 cargo build -j 1' 'taskset -c 3 $CMD test' \
  "taskset -c 3 bash -s <<< 'cargo test'" 'nix develop --help' 'nix develop --expr 1 -c cargo fmt' 'cargo -q fmt --check' 'cargo +nightly fmt' \
  'nix develop -c taskset -c 3 cargo test' "nix develop ../..#references -c bash -c 'taskset -c 2 dvc repro'" \
  'nix shell nixpkgs#x -c taskset -c 1 cargo test' \
  'taskset -c 3 target/release/xs in.toml' 'taskset -c 3 nix develop -c cargo test' 'taskset -c 3 cargo test' \
  'taskset -c 3 bin/xs in.toml' 'taskset --cpu-list 2 dvc repro' 'taskset 0x4 cargo build' 'taskset -c 5-5 nix build .#x' \
  'sudo taskset -c 3 cargo test' "taskset -c 3 bash -c 'cargo test'"; do
  out=$(PATH="$FAKE_SQUEUE/exclusive:$PATH" run_hook guard-exclusive-slurm "$cmd")
  assert_eq "" "$out"
done

it "guard-sbatch: nix develop|shell -c の中の sbatch も止める"
for cmd in 'nix develop -c sbatch job.sh' 'nix shell nixpkgs#x -c sbatch job.sh' 'nix develop --option k v -c sbatch job.sh'; do
  out=$(run_hook guard-sbatch "$cmd")
  assert_eq deny "$(decision "$out")"
done
out=$(run_hook guard-sbatch 'nix develop --help')
assert_eq "" "$out"

it "guard-exclusive-slurm: 対象のコマンド以外は通す"
for cmd in 'cargo fmt --check' 'cargo --version' 'nix flake show' 'nix develop' 'nix develop -c cargo fmt' 'nix shell nixpkgs#x -c ls' 'nix develop -c bash -c "echo hi"' 'dvc status' 'git status' 'ls bin/' 'echo cargo test' \
  'bin/other' 'target/release/other' 'squeue -t R'; do
  out=$(PATH="$FAKE_SQUEUE/exclusive:$PATH" run_hook guard-exclusive-slurm "$cmd")
  assert_eq "" "$out"
done

it "guard-exclusive-slurm: squeue の出力が大きくても（パイプを満たしても）排他のジョブを見つける"
out=$(PATH="$FAKE_SQUEUE/many:$PATH" run_hook guard-exclusive-slurm 'cargo test')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "99999"

it "guard-exclusive-slurm: 排他のジョブが無い（排他でないジョブだけ、ジョブなし）なら通す"
for dir in shared idle; do
  out=$(PATH="$FAKE_SQUEUE/$dir:$PATH" run_hook guard-exclusive-slurm 'cargo test')
  assert_eq "" "$out"
done

finish
