#!/usr/bin/env bash
# PreToolUse hook（modules/home/parts/claude-hooks、Rust 製 1 バイナリ）のルールごとのテスト
#
# 実行: bash .claude/tests/hooks.sh
#   lib.sh が cargo build してから各ルールを --rule で個別に評価する
# 対象: pre-pr-create-check.sh / warn-large-commit.sh / guard-git-push.sh / pre-merge-check.sh /
#       block-secret-commit.sh / guard-git-add.sh / guard-gh-run-rerun.sh / guard-gh-api.sh

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
# guard-git-push.sh: feature branch への --force-with-lease は open PR があっても許可
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
out=$(run_hook guard-git-push.sh 'git push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push.sh 'git push')
assert_eq allow "$(decision "$out")"

it "guard-git-push: open PR のあるブランチへの --force-with-lease も許可し、gh に問い合わせない"
make_fake_gh '"pr view feat/x"*) echo OPEN ;;'
out=$(run_hook guard-git-push.sh 'git push --force-with-lease')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-push.sh 'git push --force-with-lease origin feat/x')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-git-push: --force-with-lease でも main 宛ては deny"
out=$(run_hook guard-git-push.sh 'git push --force-with-lease origin main')
assert_eq deny "$(decision "$out")"

it "guard-git-push: --force / +refspec は feature branch でも deny"
out=$(run_hook guard-git-push.sh 'git push --force')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "--force-with-lease"
out=$(run_hook guard-git-push.sh 'git push origin +feat/x')
assert_eq deny "$(decision "$out")"

it "guard-git-push: ALLOW_PROTECTED_PUSH=1 で迂回できる"
out=$(run_hook guard-git-push.sh 'ALLOW_PROTECTED_PUSH=1 git push --force')
assert_eq allow "$(decision "$out")"

it "guard-git-push: GitHub リモートがなければ gh を呼ばず許可"
LOCAL="$TEST_ROOT/push-local"
make_repo "$LOCAL"
make_remote "$LOCAL"
git -C "$LOCAL" switch -q -c feat/x
cd "$LOCAL" || exit 1
make_fake_gh ''
out=$(run_hook guard-git-push.sh 'git push origin main')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

# ===========================================================================
# block-main-commit.sh: flake.lock だけの更新は main でも通す
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
out=$(run_hook block-main-commit.sh 'git commit -m "chore(flake): update (r995)"')
assert_eq allow "$(decision "$out")"

it "block-main-commit: flake.lock 以外が混ざっていれば main では deny"
echo 'y' > other.txt
git add other.txt
out=$(run_hook block-main-commit.sh 'git commit -m "chore: mixed"')
assert_eq deny "$(decision "$out")"
git restore --staged other.txt && git checkout -q other.txt

it "block-main-commit: flake.lock の削除・リネームは main では deny"
git rm -q --cached flake.lock
out=$(run_hook block-main-commit.sh 'git commit -m "chore(flake): remove"')
assert_eq deny "$(decision "$out")"
git restore --staged flake.lock

it "block-main-commit: 何もステージされていなければ main では deny"
git restore --staged flake.lock 2>/dev/null || true
git checkout -q flake.lock
out=$(run_hook block-main-commit.sh 'git commit -m "chore: nothing"')
assert_eq deny "$(decision "$out")"

it "block-main-commit: feature ブランチでは従来どおり何でも通す"
git switch -q -c feat/x
echo 'z' > other.txt && git add other.txt
out=$(run_hook block-main-commit.sh 'git commit -m "feat: x"')
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
out=$(run_hook block-main-commit.sh 'git commit -m "feat: x"')
assert_eq allow "$(decision "$out")"

it "block-main-commit: GitHub 以外のリモートだけならローカル専用と同じ扱い"
make_remote "$LOCAL"
out=$(run_hook block-main-commit.sh 'git commit -m "feat: x"')
assert_eq allow "$(decision "$out")"

it "guard-git-push: GitHub リモートがなければ main への push も force push も止めない"
# 直前の make_remote でローカルの bare リモートだけがある状態
BARE="$TEST_ROOT/localonly.git"
out=$(run_hook guard-git-push.sh "git push origin main")
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-push.sh "git push --force origin main")
assert_eq allow "$(decision "$out")"
assert_file_exists "$BARE"

it "block-main-commit / guard-git-push: GitHub リモートがあれば従来どおり止める"
git remote add gh https://github.com/example/localonly.git
out=$(run_hook block-main-commit.sh 'git commit -m "feat: x"')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push.sh "git push origin main")
assert_eq deny "$(decision "$out")"
git remote remove gh

it "block-main-commit: -C で指定した別リポジトリのリモートで判定する"
cd "$TEST_ROOT" || exit 1
out=$(run_hook block-main-commit.sh "git -C $LOCAL commit -m 'feat: x'")
assert_eq allow "$(decision "$out")"
out=$(run_hook block-main-commit.sh "git -C $TEST_ROOT/lock commit -m 'feat: x'")
assert_eq deny "$(decision "$out")"
cd "$REPO" || exit 1

it "block-main-commit: コマンドより後ろの cd は対象ディレクトリにしない"
# $REPO（github リモートあり・main）で実行。後ろの cd $LOCAL（リモートなし）を見てしまうと allow になる
out=$(run_hook block-main-commit.sh "git commit -m 'feat: x' && cd $LOCAL")
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

it "heredoc: 算術式のシフト演算子をヒアドキュメントの開始とみなさない"
# $(( 1 << 3 )) の << はシフト演算子。これを開始と誤認すると終端子が現れず、
# 以降の行がすべて本文としてマスクされ、実コマンドが hook から見えなくなる
out=$(run_hook guard-git-push.sh 'SIZE=$(( 1 << 3 ))
git push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push.sh 'if (( n << 2 )); then :; fi
git push origin main')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push.sh 'if (( n << foo &&
 m )); then :; fi
git push origin main')
assert_eq deny "$(decision "$out")"

it "heredoc: クォートの中の << はヒアドキュメントの開始ではない（bash 版の既知の限界）"
out=$(run_hook guard-git-push.sh 'echo "a << b"; git push origin main')
assert_eq deny "$(decision "$out")"

it "heredoc: ハイフン入りの終端子でも本文の後ろの実コマンドは検出する"
out=$(run_hook guard-git-push.sh "cat > doc.md <<END-TEXT
例: git push origin main は禁止
END-TEXT
git push origin main")
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-git-push.sh "cat > doc.md <<123
例: git push origin main は禁止
123")
assert_eq allow "$(decision "$out")"

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
# make_fake_gh_merge <behind_by>: マージ可能な PR #1 の gh 応答。compare は base...head の遅れを返す
make_fake_gh_merge() {
  make_fake_gh '"pr view --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "pr view 1 --json number,headRefOid,reviewDecision,baseRefName"*) echo "{\"number\":1,\"headRefOid\":\"abc\",\"reviewDecision\":\"\",\"baseRefName\":\"main\"}" ;;
  "repo view --json owner"*) echo example ;;
  "repo view --json name"*) echo heredoc ;;
  "api --paginate repos/example/heredoc/commits/abc/check-runs"*) echo "{\"name\":\"ci\",\"status\":\"completed\",\"conclusion\":\"success\"}" ;;
  "api repos/example/heredoc/commits/abc/status"*) echo "[]" ;;
  "api repos/example/heredoc/compare/main...abc"*) '"$1"' ;;
  "api graphql"*) echo "[]" ;;'
}
make_fake_gh_merge 'echo 0'
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

it "heredoc: 本文中の見出しでゲートを通せない（見出し検査は実際に渡す本文だけを見る）"
# ヒアドキュメント本文に「見出しの揃った例」を書いておき、実際の PR 本文には
# 見出しを書かない。本文検査が本文の外まで拾うと、これで deny をすり抜けられる
DECOY_CREATE="cat > doc.md <<'SENTINEL'
gh pr create --title t --body \"## 概要 x ## 変更点 y ## テスト z\"
SENTINEL
gh pr create --title \"feat: x\" --body \"見出しなし\""
out=$(run_hook pre-pr-create-check.sh "$DECOY_CREATE")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## 概要"

DECOY_MERGE="cat > doc.md <<'SENTINEL'
gh pr merge 1 --merge --delete-branch --body \"## Why x ## What y ## Impact z\"
SENTINEL
gh pr merge 1 --merge --delete-branch --subject \"Merge: x\" --body \"見出しなし\""
out=$(run_hook pre-merge-check.sh "$DECOY_MERGE")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "## Why"

# ===========================================================================
# pre-merge-check.sh: head が base より遅れていれば deny（origin/main にリベースしてからマージする）
# ===========================================================================

it "pre-merge-check: base より遅れていれば deny し、リベースを求める"
make_fake_gh_merge 'echo 2'
out=$(run_hook pre-merge-check.sh "$MERGE_CMD")
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
  "api graphql"*) echo "[]" ;;'
out=$(run_hook pre-merge-check.sh "$MERGE_CMD")
assert_eq allow "$(decision "$out")"

it "pre-merge-check: base との差を取得できなければ deny"
make_fake_gh_merge 'echo "error connecting to api.github.com" >&2; exit 1'
out=$(run_hook pre-merge-check.sh "$MERGE_CMD")
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
out=$(run_hook block-secret-commit.sh 'git commit -m "feat: a"')
assert_eq allow "$(decision "$out")"
assert_eq "git --staged --no-banner --redact --report-format json --report-path - ." "$(fake_log gitleaks)"

it "block-secret-commit: git commit 以外は対象外で gitleaks を呼ばない"
fake_gitleaks ok ok
out=$(run_hook block-secret-commit.sh 'git log --grep commit')
assert_eq "" "$out"
assert_eq "" "$(fake_log gitleaks)"

it "block-secret-commit: 漏えいがあれば deny し、場所とルール ID を示す（値は出さない）"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit.sh 'git commit -m "chore: conf"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "conf.txt:3"
assert_contains "$(reason "$out")" "aws-access-token"
assert_contains "$(reason "$out")" "gitleaks:allow"
assert_not_contains "$(reason "$out")" "REDACTED"

it "block-secret-commit: -a 指定時は未ステージの変更（--pre-commit）も検査する"
fake_gitleaks ok "$LEAK_JSON"
out=$(run_hook block-secret-commit.sh 'git commit -am "chore: a"')
assert_eq deny "$(decision "$out")"
assert_contains "$(fake_log gitleaks)" "git --pre-commit"
fake_gitleaks ok "$LEAK_JSON"
out=$(run_hook block-secret-commit.sh 'git commit -m "chore: staged only"')
assert_eq allow "$(decision "$out")"
assert_not_contains "$(fake_log gitleaks)" "git --pre-commit"

it "block-secret-commit: gitleaks が実行できなければ deny（検査できない状態でコミットさせない）"
fake_gitleaks fail ok
out=$(run_hook block-secret-commit.sh 'git commit -m "chore: x"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "gitleaks"
remove_fake_tool gitleaks
out=$(PATH="$(minimal_path)" run_hook block-secret-commit.sh 'git commit -m "chore: x"')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "gitleaks"

it "block-secret-commit: ALLOW_SECRET_COMMIT=1 で迂回できる（gitleaks を呼ばない）"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit.sh 'ALLOW_SECRET_COMMIT=1 git commit -m "chore: conf"')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(fake_log gitleaks)"

it "block-secret-commit: -C 指定のリポジトリで gitleaks を実行する"
fake_gitleaks ok ok
cd "$TEST_ROOT" || exit 1
out=$(run_hook block-secret-commit.sh "git -C $REPO commit -m 'chore: c'")
assert_eq allow "$(decision "$out")"
assert_eq "$REPO" "$(cat "$TEST_ROOT/gitleaks.pwd")"
cd "$REPO" || exit 1

it "block-secret-commit: ヒアドキュメント本文の git commit には反応しない"
fake_gitleaks "$LEAK_JSON" ok
out=$(run_hook block-secret-commit.sh "$(write_doc '例: git commit -m msg')")
assert_eq "" "$out"
assert_eq "" "$(fake_log gitleaks)"

# ===========================================================================
# guard-git-add.sh: 対象を絞らない git add（-A / --all / . / :/ / *）を止める
# ===========================================================================
# 1 コミット 1 論理変更と、機密情報の混入防止（block-secret-commit.sh と対）のため。
# パスで範囲を限定した -A（git add -A src/）と、追跡済みだけを対象にする -u は通す

REPO="$TEST_ROOT/gitadd"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1
mkdir -p src && echo x > src/a.txt

it "guard-git-add: パスを指定した git add は許可する"
for cmd in 'git add src/a.txt' 'git add -p' 'git add -u' 'git add ./src' 'git add .claude/' 'git add -- src/a.txt'; do
  out=$(run_hook guard-git-add.sh "$cmd")
  assert_eq allow "$(decision "$out")"
done

it "guard-git-add: -A / --all / . / :/ / * は deny し、代わりの手順を示す"
for cmd in 'git add -A' 'git add --all' 'git add .' 'git add -A .' 'git add :/' 'git add *' 'git add -An'; do
  out=$(run_hook guard-git-add.sh "$cmd")
  assert_eq deny "$(decision "$out")"
done
assert_contains "$(reason "$out")" "git add -p"

it "guard-git-add: 文字列引数の中の git add -A はコマンドではない（bash 版の誤検出）"
out=$(run_hook guard-git-add.sh 'git commit -m "docs: run git add -A"')
assert_eq allow "$(decision "$out")"

it "guard-git-add: パスで範囲を限定した -A は許可する"
out=$(run_hook guard-git-add.sh 'git add -A src/')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-git-add.sh 'git add --all -- src/')
assert_eq allow "$(decision "$out")"

it "guard-git-add: git add 以外は対象外"
out=$(run_hook guard-git-add.sh 'git commit -am "feat: x"')
assert_eq "" "$out"
out=$(run_hook guard-git-add.sh 'git log --grep "add -A"')
assert_eq "" "$out"

it "guard-git-add: 連結コマンドの 2 つ目以降も検査する"
out=$(run_hook guard-git-add.sh 'git add src/a.txt && git add -A')
assert_eq deny "$(decision "$out")"

it "guard-git-add: -C で指定した別リポジトリでも検査する"
cd "$TEST_ROOT" || exit 1
out=$(run_hook guard-git-add.sh "git -C $REPO add -A")
assert_eq deny "$(decision "$out")"
cd "$REPO" || exit 1

it "guard-git-add: ALLOW_GIT_ADD_ALL=1 で迂回できる"
out=$(run_hook guard-git-add.sh 'ALLOW_GIT_ADD_ALL=1 git add -A')
assert_eq allow "$(decision "$out")"

it "guard-git-add: ヒアドキュメント本文の git add -A には反応しない"
out=$(run_hook guard-git-add.sh "$(write_doc 'git add -A は禁止')")
assert_eq allow "$(decision "$out")"

# ===========================================================================
# guard-gh-run-rerun.sh: 既に再実行済みの run の gh run rerun を止める
# ===========================================================================
# 一時障害の再実行は 1 回まで。attempt 2 以上で同じ失敗なら一時障害ではないので、
# 無制限に再実行せず原因を報告してユーザーの判断に委ねる

cd "$TEST_ROOT" || exit 1

it "guard-gh-run-rerun: 初回の失敗（attempt 1）の再実行は許可する"
make_fake_gh '"run view 100 --json attempt"*) echo "{\"attempt\":1}" ;;'
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun 100')
assert_eq allow "$(decision "$out")"
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun 100 --failed')
assert_eq allow "$(decision "$out")"

it "guard-gh-run-rerun: attempt 2 以上の run は deny し、報告に切り替えるよう示す"
make_fake_gh '"run view 100 --json attempt"*) echo "{\"attempt\":2}" ;;'
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun 100')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "2 回"
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun --failed 100')
assert_eq deny "$(decision "$out")"

it "guard-gh-run-rerun: -R 指定のリポジトリで attempt を確認する"
make_fake_gh '"run view -R octo/repo 100 --json attempt"*) echo "{\"attempt\":3}" ;;'
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun -R octo/repo 100')
assert_eq deny "$(decision "$out")"

it "guard-gh-run-rerun: --job の値（数値のジョブ ID）を run ID と取り違えない"
make_fake_gh '"run view 100 --json attempt"*) echo "{\"attempt\":1}" ;;'
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun --job 555 100')
assert_eq allow "$(decision "$out")"
assert_contains "$(cat "$FAKE_GH_LOG")" "run view 100 --json attempt"
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun 100 -j 555')
assert_eq allow "$(decision "$out")"

it "guard-gh-run-rerun: attempt を確認できなければ deny"
make_fake_gh '"run view 100 --json attempt"*) echo "not found" >&2; exit 1 ;;'
out=$(run_hook guard-gh-run-rerun.sh 'gh run rerun 100')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "確認できません"

it "guard-gh-run-rerun: gh run rerun 以外は対象外で gh を呼ばない"
make_fake_gh ''
out=$(run_hook guard-gh-run-rerun.sh 'gh run view 100 --log-failed')
assert_eq "" "$out"
out=$(run_hook guard-gh-run-rerun.sh 'gh run list')
assert_eq "" "$out"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-gh-run-rerun: ALLOW_RERUN=1 で迂回できる（gh を呼ばない）"
out=$(run_hook guard-gh-run-rerun.sh 'ALLOW_RERUN=1 gh run rerun 100')
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

it "guard-gh-run-rerun: ヒアドキュメント本文の gh run rerun には反応しない"
out=$(run_hook guard-gh-run-rerun.sh "$(write_doc '再実行: gh run rerun 100')")
assert_eq allow "$(decision "$out")"
assert_eq "" "$(cat "$FAKE_GH_LOG")"

# ===========================================================================
# guard-gh-api.sh: 生の gh api で行ってはいけない操作を止める
# ===========================================================================
# 1. Actions の権限設定（actions/permissions 配下）への書き込み。CI の権限エラーを
#    リポジトリ設定 default_workflow_permissions の緩和で回避させない
# 2. GraphQL の resolveReviewThread / unresolveReviewThread。resolve は resolve-thread.sh
#    経由に固定し、人間のレビュアーのスレッドを勝手に閉じない判定を迂回できなくする

it "guard-gh-api: actions/permissions の読み取り（GET）は許可する"
out=$(run_hook guard-gh-api.sh 'gh api repos/octo/repo/actions/permissions/workflow')
assert_eq allow "$(decision "$out")"

it "guard-gh-api: actions/permissions への書き込みは deny し、permissions: の宣言を示す"
out=$(run_hook guard-gh-api.sh 'gh api -X PUT repos/octo/repo/actions/permissions/workflow -f default_workflow_permissions=write')
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "permissions:"
out=$(run_hook guard-gh-api.sh 'gh api --method PATCH repos/octo/repo/actions/permissions')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-gh-api.sh 'gh api repos/octo/repo/actions/permissions/workflow -f default_workflow_permissions=write')
assert_eq deny "$(decision "$out")"
out=$(run_hook guard-gh-api.sh 'gh api "repos/{owner}/{repo}/actions/permissions/workflow" --input body.json')
assert_eq deny "$(decision "$out")"

it "guard-gh-api: GraphQL の resolveReviewThread は deny し、resolve-thread.sh を示す"
out=$(run_hook guard-gh-api.sh "gh api graphql -f query='mutation { resolveReviewThread(input: {threadId: \"x\"}) { thread { id } } }'")
assert_eq deny "$(decision "$out")"
assert_contains "$(reason "$out")" "resolve-thread.sh"
out=$(run_hook guard-gh-api.sh "gh api graphql -f query='mutation { unresolveReviewThread(input: {threadId: \"x\"}) { thread { id } } }'")
assert_eq deny "$(decision "$out")"

it "guard-gh-api: ヒアドキュメントで渡した mutation も deny する（本文が操作そのもの）"
out=$(run_hook guard-gh-api.sh "gh api graphql -f query=\"\$(cat <<'EOF'
mutation { resolveReviewThread(input: {threadId: \"x\"}) { thread { id } } }
EOF
)\"")
assert_eq deny "$(decision "$out")"

it "guard-gh-api: reviewThreads の読み取りクエリは許可する"
out=$(run_hook guard-gh-api.sh "gh api graphql -f query='query { repository(owner: \"o\", name: \"r\") { pullRequest(number: 1) { reviewThreads(first: 100) { nodes { id isResolved } } } } }'")
assert_eq allow "$(decision "$out")"

it "guard-gh-api: resolve-thread.sh の実行と gh api 以外は対象外"
out=$(run_hook guard-gh-api.sh '~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh 1 2')
assert_eq "" "$out"
out=$(run_hook guard-gh-api.sh 'gh pr view 1')
assert_eq "" "$out"

it "guard-gh-api: 説明文の中の resolveReviewThread や actions/permissions には反応しない"
out=$(run_hook guard-gh-api.sh "$(write_doc 'resolveReviewThread は gh api で叩かない。actions/permissions も PUT しない')")
assert_eq allow "$(decision "$out")"

finish
