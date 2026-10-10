#!/usr/bin/env bash
# PreToolUse hook（modules/home/parts/claude-hooks、Rust 製 1 バイナリ）のルールごとのテスト
#
# 実行: bash .claude/tests/hooks.sh
#   claude-hooks を cargo build してから各ルールを --rule で個別に評価する
# 対象: pre-pr-create-check / warn-large-commit / guard-git-push / pre-merge-check /
#       block-secret-commit / guard-git-add / guard-gh-run-rerun / guard-gh-api /
#       block-main-commit / require-background-wait / guard-branch-base / pre-git-merge-check /
#       guard-git-merge

# hook はリポジトリの Rust クレート（claude-hooks）をビルドした 1 バイナリ。
# lib.sh が HOME を差し替える前にビルドする（~/.cargo/config.toml の sccache 等を使うため）。
# CLAUDE_HOOKS_BIN で差し替えられる（nix でビルドしたバイナリの検証用）
if [[ -z "${CLAUDE_HOOKS_BIN:-}" ]]; then
  CRATE_DIR=$(cd "$(dirname "$0")/../../modules/home/parts/claude-hooks" && pwd)
  cargo build -q --manifest-path "$CRATE_DIR/Cargo.toml" || exit 1
  CLAUDE_HOOKS_BIN="$CRATE_DIR/target/debug/claude-hooks"
fi

source "$(dirname "$0")/lib.sh"

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

it "pre-pr-create: 未プッシュコミットがあれば deny"
commit_file "$REPO" "b.txt" "feat: b"
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "未プッシュ"
git -C "$REPO" push -q

it "pre-pr-create: 上流ブランチがなければ deny"
git -C "$REPO" switch -q -c feat/no-upstream
out=$(run_hook pre-pr-create-check "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git push"
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

it "guard-git-push: ALLOW_PROTECTED_PUSH=1 で迂回できる"
out=$(run_hook guard-git-push 'ALLOW_PROTECTED_PUSH=1 git push --force')
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
# 作業が進まない。判定は gh-wait-review.sh と同じ「GitHub リモートがあるか」に揃える。
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
cd "$REPO" || exit 1

it "block-main-commit: コマンド置換やサブシェルの中の cd は親の対象ディレクトリを変えない"
out=$(run_hook block-main-commit "echo \$(cd $LOCAL && pwd) && git commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"
out=$(run_hook block-main-commit "(cd $LOCAL); git commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"

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
# block-secret-commit: 機密情報を含む変更のコミットを gitleaks で止める
# ===========================================================================
# 検出そのものは自前で持たず gitleaks に委ねる（既存の定番ツールを使う）。hook が担うのは
# 「いつ・何を渡して呼ぶか」「結果をどう deny に変換するか」だけなので、偽の gitleaks で
# 呼び出しと結果の扱いを検証する。値は理由に出さない（gitleaks 側でも --redact する）

REPO="$TEST_ROOT/secret"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1
echo 'plain' > a.txt && git add a.txt

LEAK_JSON='[{"File":"conf.txt","StartLine":3,"RuleID":"aws-access-token","Secret":"REDACTED","Fingerprint":"conf.txt:aws-access-token:3"}]'
# fake_gitleaks <staged-json|exit-code> <pre-commit-json|exit-code>
#   JSON を渡すと exit 1（漏えいあり）、"ok" なら [] と exit 0、"fail" なら JSON なしで exit 2
fake_gitleaks() {
  local body="" mode
  for mode in "--staged:$1" "--pre-commit:$2"; do
    local flag="${mode%%:*}" spec="${mode#*:}"
    case "$spec" in
      ok)   body+="\"git $flag --no-banner --redact --report-format json --report-path - .\"*) pwd >> \"$TEST_ROOT/gitleaks.pwd\"; echo '[]' ;;"$'\n' ;;
      fail) body+="\"git $flag --no-banner --redact --report-format json --report-path - .\"*) echo 'error: not a git repo' >&2; exit 2 ;;"$'\n' ;;
      *)    body+="\"git $flag --no-banner --redact --report-format json --report-path - .\"*) pwd >> \"$TEST_ROOT/gitleaks.pwd\"; echo '$spec'; exit 1 ;;"$'\n' ;;
    esac
  done
  : > "$TEST_ROOT/gitleaks.pwd"
  make_fake_tool gitleaks "$body"
}

it "block-secret-commit: gitleaks がステージ済みに漏えいを見つけなければ許可する"
fake_gitleaks ok ok
out=$(run_hook block-secret-commit 'git commit -m "feat: a"')
assert_eq allow "$(decision "$out")"
assert_eq "git --staged --no-banner --redact --report-format json --report-path - ." "$(fake_log gitleaks)"

it "block-secret-commit: git commit 以外は対象外で gitleaks を呼ばない"
fake_gitleaks ok ok
out=$(run_hook block-secret-commit 'git log --grep commit')
assert_eq "" "$out"
assert_eq "" "$(fake_log gitleaks)"

it "block-secret-commit: 漏えいがあれば deny し、場所とルール ID を示す（値は出さない）"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit 'git commit -m "chore: conf"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "conf.txt:3"
assert_contains "$(reason "$out")" "aws-access-token"
assert_contains "$(reason "$out")" "gitleaks:allow"
assert_not_contains "$(reason "$out")" "REDACTED"

it "block-secret-commit: -a 指定時は未ステージの変更（--pre-commit）も検査する"
fake_gitleaks ok "$LEAK_JSON"
out=$(run_hook block-secret-commit 'git commit -am "chore: a"')
assert_eq deny "$(decision "$out")"
assert_contains "$(fake_log gitleaks)" "git --pre-commit"
fake_gitleaks ok "$LEAK_JSON"
out=$(run_hook block-secret-commit 'git commit -m "chore: staged only"')
assert_eq allow "$(decision "$out")"
assert_not_contains "$(fake_log gitleaks)" "git --pre-commit"

it "block-secret-commit: gitleaks が実行できなければ deny（検査できない状態でコミットさせない）"
fake_gitleaks fail ok
out=$(run_hook block-secret-commit 'git commit -m "chore: x"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "gitleaks"
remove_fake_tool gitleaks
out=$(PATH="$(minimal_path)" run_hook block-secret-commit 'git commit -m "chore: x"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "gitleaks"

it "block-secret-commit: ALLOW_SECRET_COMMIT=1 で迂回できる（gitleaks を呼ばない）"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit 'ALLOW_SECRET_COMMIT=1 git commit -m "chore: conf"')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(fake_log gitleaks)"

it "block-secret-commit: -C 指定のリポジトリで gitleaks を実行する"
fake_gitleaks ok ok
cd "$TEST_ROOT" || exit 1
out=$(run_hook block-secret-commit "git -C $REPO commit -m 'chore: c'")
assert_eq allow "$(decision "$out")"
assert_eq "$REPO" "$(cat "$TEST_ROOT/gitleaks.pwd")"
cd "$REPO" || exit 1

it "block-secret-commit: ヒアドキュメント本文の git commit には反応しない"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit "$(write_doc '例: git commit -m msg')")
assert_eq "" "$out"
assert_eq "" "$(fake_log gitleaks)"

# ===========================================================================
# guard-git-add: 対象を絞らない git add（-A / --all / . / :/ / *）を止める
# ===========================================================================
# 1 コミット 1 論理変更と、機密情報の混入防止（block-secret-commit と対）のため。
# パスで範囲を限定した -A（git add -A src/）と、追跡済みだけを対象にする -u は通す

REPO="$TEST_ROOT/gitadd"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1
mkdir -p src && echo x > src/a.txt

it "guard-git-add: パスを指定した git add は許可する"
for cmd in 'git add src/a.txt' 'git add -p' 'git add -u' 'git add ./src' 'git add .claude/' 'git add -- src/a.txt'; do
  out=$(run_hook guard-git-add "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-git-add: -A / --all / . / :/ / * は deny し、代わりの手順を示す"
for cmd in 'git add -A' 'git add --all' 'git add .' 'git add -A .' 'git add :/' 'git add *' 'git add -An'; do
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

it "guard-git-add: ALLOW_GIT_ADD_ALL=1 で迂回できる"
out=$(run_hook guard-git-add 'ALLOW_GIT_ADD_ALL=1 git add -A')
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

it "guard-gh-run-rerun: gh run rerun 以外は対象外で gh を呼ばない"
make_fake_gh ''
out=$(run_hook guard-gh-run-rerun 'gh run view 100 --log-failed')
assert_eq "" "$out"
out=$(run_hook guard-gh-run-rerun 'gh run list')
assert_eq "" "$out"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-gh-run-rerun: ALLOW_RERUN=1 で迂回できる（gh を呼ばない）"
out=$(run_hook guard-gh-run-rerun 'ALLOW_RERUN=1 gh run rerun 100')
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
assert_contains "$(reason "$out")" "resolve-thread.sh"
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
assert_contains "$(reason "$out")" "request-rereview.sh"

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
assert_contains "$(reason "$out")" "ALLOW_UNREVIEWED_HEAD=1"
assert_not_contains "$(reason "$out")" "失敗したチェックがあります"

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

it "pre-merge-check: ALLOW_UNREVIEWED_HEAD=1 なら未レビューの head でもマージできる"
out=$(run_hook pre-merge-check "ALLOW_UNREVIEWED_HEAD=1 $MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: レビューは走っているのに未レビューなら、まず要求を促す"
make_fake_gh_merge 'echo 0' 'echo old'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "request-rereview.sh"

it "pre-merge-check: 挙動を変えない修正に限る条件付きでエスケープを案内する"
# typo 1 文字の修正にレビュー 1 周（数分＋レビュー用トークン）は見合わない。
# ただし条件を書かずに案内すると迂回路になるので、対象と記録の要件を明示する
make_fake_gh_merge 'echo 0' 'echo old'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "挙動を変えない"
assert_contains "$(reason "$out")" "ALLOW_UNREVIEWED_HEAD=1"
assert_contains "$(reason "$out")" "run-checks.sh"
assert_contains "$(reason "$out")" "ALL_OK"
# run-checks.sh はツール未導入なら SKIP でも ALL_OK になるので、テストの通過も条件
assert_contains "$(reason "$out")" "既存テスト"
assert_contains "$(reason "$out")" "完了報告"

it "pre-merge-check: 周回上限に達したときはユーザーの承認を条件にエスケープを案内する"
# 上限到達後は再レビューを依頼できないので、案内が「依頼」と「挙動を変えない修正」だけだと
# ユーザーが承認しても次の一手がない
make_fake_gh_merge 'echo 0' 'echo old'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_contains "$(reason "$out")" "周回上限"
assert_contains "$(reason "$out")" "ユーザーが承認"

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

it "guard-branch-base: 起点のない git switch -c / -C は deny し、理由と直し方を示す"
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
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC=1"
assert_not_contains "$(reason "$out")" "src/x.rs"

it "pre-merge-check: 新しい ADR が 1 件で、ほかは既存の ADR の変更・改名・README なら許可する"
make_fake_gh_merge 'echo 0' 'echo abc' 'printf "%s\t%s\n" added docs/adr/0002-use-b.md modified docs/adr/0001-keep-a.md renamed docs/adr/0004-new-name.md added docs/adr/README.md added docs/adr/drafts/0005-x.md added docs/adrs/0006-y.md'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: ALLOW_MULTI_TOPIC=1 なら新しい ADR が複数でもマージできる"
make_fake_gh_merge 'echo 0' 'echo abc' "$TWO_ADRS"
out=$(run_hook pre-merge-check "ALLOW_MULTI_TOPIC=1 $MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: PR の変更ファイルを取得できなければ deny"
make_fake_gh_merge 'echo 0' 'echo abc' 'echo "error connecting to api.github.com" >&2; exit 1'
out=$(run_hook pre-merge-check "$MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "変更ファイル"

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
assert_contains "$(reason "$out")" "ALLOW_MULTI_TOPIC=1"
out=$(run_hook pre-git-merge-check 'git merge -m "Merge: two" --no-ff feat/two')
assert_eq deny "$(decision "$out")"

it "pre-git-merge-check: 既存の ADR の status の変化と README は数えない"
out=$(run_hook pre-git-merge-check 'git merge --no-ff feat/one')
assert_eq allow "$(decision "$out")"

it "pre-git-merge-check: ALLOW_MULTI_TOPIC=1 で通す"
out=$(run_hook pre-git-merge-check 'ALLOW_MULTI_TOPIC=1 git merge --no-ff feat/two')
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

finish
