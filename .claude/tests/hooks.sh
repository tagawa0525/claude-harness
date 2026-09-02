#!/usr/bin/env bash
# .claude/hooks のテスト
#
# 実行: bash .claude/tests/hooks.sh
# 対象: pre-pr-create-check.sh / warn-large-commit.sh / guard-git-push.sh（open PR への force push）

source "$(dirname "$0")/lib.sh"

# ===========================================================================
# pre-pr-create-check.sh
# ===========================================================================

REPO="$TEST_ROOT/prcreate"
make_repo "$REPO"
make_remote "$REPO" github
git -C "$REPO" switch -q -c feat/x
commit_file "$REPO" "a.txt" "feat: a"
git -C "$REPO" push -q -u origin feat/x
cd "$REPO" || exit 1

GOOD_BODY='## 概要
x

## 変更点
- y

## テスト
- [ ] z'
GOOD_CMD="gh pr create --title \"feat: x\" --body \"\$(cat <<'EOF'
$GOOD_BODY
EOF
)\""

it "pre-pr-create: 条件を満たす gh pr create は許可する"
out=$(run_hook pre-pr-create-check.sh "$GOOD_CMD")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: gh pr create 以外は対象外"
out=$(run_hook pre-pr-create-check.sh "gh pr view 12")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: --web は deny"
out=$(run_hook pre-pr-create-check.sh "$GOOD_CMD --web")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--web"

it "pre-pr-create: --title がなければ deny"
out=$(run_hook pre-pr-create-check.sh "gh pr create --body \"$GOOD_BODY\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--title"

it "pre-pr-create: --title が 70 文字を超えたら deny（文字数は UTF-8 の文字単位）"
long_title=$(printf 'あ%.0s' $(seq 71))
out=$(run_hook pre-pr-create-check.sh "gh pr create --title \"$long_title\" --body \"$GOOD_BODY\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "70"
ok_title=$(printf 'あ%.0s' $(seq 70))
out=$(run_hook pre-pr-create-check.sh "gh pr create --title \"$ok_title\" --body \"$GOOD_BODY\"")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: -t の短縮形も title として認識する"
out=$(run_hook pre-pr-create-check.sh "gh pr create -t \"feat: x\" -b \"$GOOD_BODY\"")
assert_eq allow "$(decision "$out")"

it "pre-pr-create: --body / --body-file がなければ deny（--fill は本文とみなさない）"
out=$(run_hook pre-pr-create-check.sh "gh pr create --title \"feat: x\" --fill")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--body"

it "pre-pr-create: 本文に見出しが欠けていたら欠けた見出しを列挙して deny"
out=$(run_hook pre-pr-create-check.sh "gh pr create --title \"feat: x\" --body \"## 概要
x\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## 変更点"
assert_contains "$(reason "$out")" "## テスト"
assert_not_contains "$(reason "$out")" "## 概要"

it "pre-pr-create: --body-file の内容で見出しを判定する"
printf '%s\n' "$GOOD_BODY" > "$TEST_ROOT/body.md"
out=$(run_hook pre-pr-create-check.sh "gh pr create --title \"feat: x\" --body-file $TEST_ROOT/body.md")
assert_eq allow "$(decision "$out")"
echo "## 概要" > "$TEST_ROOT/bad-body.md"
out=$(run_hook pre-pr-create-check.sh "gh pr create --title \"feat: x\" -F $TEST_ROOT/bad-body.md")
assert_eq deny "$(decision "$out")"

it "pre-pr-create: 未プッシュコミットがあれば deny"
commit_file "$REPO" "b.txt" "feat: b"
out=$(run_hook pre-pr-create-check.sh "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "未プッシュ"
git -C "$REPO" push -q

it "pre-pr-create: 上流ブランチがなければ deny"
git -C "$REPO" switch -q -c feat/no-upstream
out=$(run_hook pre-pr-create-check.sh "$GOOD_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "git push"
git -C "$REPO" switch -q feat/x

it "pre-pr-create: --head 指定時は現在ブランチの push 状態を見ない"
git -C "$REPO" switch -q feat/no-upstream
out=$(run_hook pre-pr-create-check.sh "$GOOD_CMD --head feat/x")
assert_eq allow "$(decision "$out")"
git -C "$REPO" switch -q feat/x

it "pre-pr-create: cd 先のリポジトリで判定する"
OTHER="$TEST_ROOT/prcreate-other"
make_repo "$OTHER"
git -C "$OTHER" switch -q -c feat/y
cd "$TEST_ROOT" || exit 1
out=$(run_hook pre-pr-create-check.sh "cd $OTHER && $GOOD_CMD")
assert_eq deny "$(decision "$out")"
cd "$REPO" || exit 1

# ===========================================================================
# warn-large-commit.sh
# ===========================================================================

REPO="$TEST_ROOT/large"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1

it "warn-large-commit: 小さな変更では何も出さない"
seq 3 > small.txt && git add small.txt
out=$(run_hook warn-large-commit.sh 'git commit -m "feat: small"')
assert_eq "" "$out"
git commit -q -m "feat: small"

it "warn-large-commit: git commit 以外は対象外"
out=$(run_hook warn-large-commit.sh 'git log --grep commit')
assert_eq "" "$out"

it "warn-large-commit: 5 ファイル以上なら additionalContext で件数を伝える（deny しない）"
for i in 1 2 3 4 5; do echo "$i" > "f$i.txt"; done
git add f1.txt f2.txt f3.txt f4.txt f5.txt
out=$(run_hook warn-large-commit.sh 'git commit -m "feat: many"')
assert_eq allow "$(decision "$out")"
assert_contains "$(additional_context "$out")" "5 ファイル"
git commit -q -m "feat: many"

it "warn-large-commit: 100 行以上なら additionalContext で行数を伝える"
seq 100 > big.txt && git add big.txt
out=$(run_hook warn-large-commit.sh 'git commit -m "feat: big"')
assert_contains "$(additional_context "$out")" "100 行"
git commit -q -m "feat: big"

it "warn-large-commit: -a 指定時は未ステージの変更も数える"
seq 200 > big.txt
out=$(run_hook warn-large-commit.sh 'git commit -am "feat: big2"')
assert_contains "$(additional_context "$out")" "行"
out=$(run_hook warn-large-commit.sh 'git commit -m "feat: nothing staged"')
assert_eq "" "$out"
git checkout -q big.txt

it "warn-large-commit: -C 指定のリポジトリを見る"
cd "$TEST_ROOT" || exit 1
seq 150 > "$REPO/c.txt" && git -C "$REPO" add c.txt
out=$(run_hook warn-large-commit.sh "git -C $REPO commit -m 'feat: c'")
assert_contains "$(additional_context "$out")" "150 行"
git -C "$REPO" commit -q -m "feat: c"

# ===========================================================================
# guard-git-push.sh: open PR のあるブランチへの force push
# ===========================================================================

REPO="$TEST_ROOT/push"
make_repo "$REPO"
make_remote "$REPO" github
git -C "$REPO" switch -q -c feat/x
commit_file "$REPO" "a.txt" "feat: a"
git -C "$REPO" push -q -u origin feat/x
cd "$REPO" || exit 1

it "guard-git-push: 既存動作 — main への push は deny、feature への通常 push は許可"
out=$(run_hook guard-git-push.sh 'git push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push.sh 'git push')
assert_eq allow "$(decision "$out")"

it "guard-git-push: open PR のあるブランチへの --force-with-lease は deny"
make_fake_gh '"pr view feat/x"*) echo OPEN ;;'
out=$(run_hook guard-git-push.sh 'git push --force-with-lease')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "open"

it "guard-git-push: refspec で指定したブランチの PR を見る"
out=$(run_hook guard-git-push.sh 'git push --force-with-lease origin feat/x')
assert_eq deny "$(decision "$out")"

it "guard-git-push: PR がなければ --force-with-lease を許可"
make_fake_gh '"pr view feat/x"*) echo "no pull requests found for branch \"feat/x\"" >&2; exit 1 ;;'
out=$(run_hook guard-git-push.sh 'git push --force-with-lease')
assert_eq allow "$(decision "$out")"

it "guard-git-push: PR が閉じていれば --force-with-lease を許可"
make_fake_gh '"pr view feat/x"*) echo MERGED ;;'
out=$(run_hook guard-git-push.sh 'git push --force-with-lease')
assert_eq allow "$(decision "$out")"

it "guard-git-push: PR の状態を確認できなければ deny"
make_fake_gh '"pr view feat/x"*) echo "error connecting to api.github.com" >&2; exit 1 ;;'
out=$(run_hook guard-git-push.sh 'git push --force-with-lease')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "確認できません"

it "guard-git-push: ALLOW_PROTECTED_PUSH=1 で迂回できる"
make_fake_gh '"pr view feat/x"*) echo OPEN ;;'
out=$(run_hook guard-git-push.sh 'ALLOW_PROTECTED_PUSH=1 git push --force-with-lease')
assert_eq allow "$(decision "$out")"

it "guard-git-push: GitHub リモートがなければ gh を呼ばず許可"
LOCAL="$TEST_ROOT/push-local"
make_repo "$LOCAL"
make_remote "$LOCAL"
git -C "$LOCAL" switch -q -c feat/x
cd "$LOCAL" || exit 1
make_fake_gh '"pr view feat/x"*) echo OPEN ;;'
out=$(run_hook guard-git-push.sh 'git push --force-with-lease')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

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
out=$(run_hook guard-git-push.sh "$(write_doc '例: git push origin main は禁止')")
assert_eq allow "$(decision "$out")"

it "heredoc: 本文中の git commit はコマンドとみなさない（block-main-commit / warn-large-commit）"
git switch -q main
seq 200 > many.txt && git add many.txt
out=$(run_hook block-main-commit.sh "$(write_doc '例: git commit -m msg')")
assert_eq allow "$(decision "$out")"
out=$(run_hook warn-large-commit.sh "$(write_doc '例: git commit -m msg')")
assert_eq "" "$out"
git reset -q && rm -f many.txt
git switch -q feat/x 2>/dev/null || git switch -q -c feat/x

it "heredoc: 本文中の gh-wait-review.sh はコマンドとみなさない（require-background-wait）"
out=$(run_hook require-background-wait.sh "$(write_doc '待機は gh-wait-review.sh を使う')")
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
out=$(run_hook pre-pr-create-check.sh "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "heredoc: 本物のコマンドは引き続き検出する（誤って全部素通しにしない）"
out=$(run_hook guard-git-push.sh "$(write_doc '例: 説明')"$'\ngit push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook pre-pr-create-check.sh "$(write_doc '例: 説明')"$'\ngh pr create --fill')
assert_eq deny "$(decision "$out")"

it "heredoc: PR 本文がヒアドキュメントでも見出しを読める（pre-pr-create）"
git -C "$REPO" push -q -u origin feat/x 2>/dev/null || true
out=$(run_hook pre-pr-create-check.sh "$GOOD_CMD")
assert_eq allow "$(decision "$out")"
out=$(run_hook pre-pr-create-check.sh "gh pr create --title \"feat: x\" --body \"\$(cat <<'EOF'
## 概要
x
EOF
)\"")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## 変更点"

it "heredoc: マージ本文がヒアドキュメントでも見出しを読める（pre-merge-check）"
make_fake_gh '"pr view --json number,headRefOid,reviewDecision"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\"}" ;;
  "pr view 1 --json number,headRefOid,reviewDecision"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo "{\"name\":\"ci\",\"status\":\"completed\",\"conclusion\":\"success\"}" ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api graphql"*) echo "[]" ;;'
out=$(run_hook pre-merge-check.sh "$MERGE_CMD")
assert_eq allow "$(decision "$out")"
BAD_MERGE_CMD="gh pr merge 1 --merge --delete-branch --subject \"Merge: x\" --body \"\$(cat <<'EOF'
## Why
x
EOF
)\""
out=$(run_hook pre-merge-check.sh "$BAD_MERGE_CMD")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## What"

finish
