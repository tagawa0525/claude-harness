#!/usr/bin/env bash
# Copilot レビューの周回（gh-pr-review/scripts/*.sh / gh-wait-review.sh） のテスト
#
# 実行: bash .claude/tests/scripts/gh-pr-review.sh（まとめて実行: bash .claude/tests/scripts.sh）
#
# 実物の gh・ruff 等は使わず、make_fake_tool で PATH 先頭に置いた偽コマンドで
# 「スクリプトが何を呼び、出力をどう判定するか」を検証する。

source "$(dirname "$0")/../lib.sh"

REVIEW_SCRIPTS="$CLAUDE_DIR/skills/gh-pr-review/scripts"

REPO="$TEST_ROOT/review/app"
make_repo "$REPO"
make_remote "$REPO" github
cd "$REPO" || exit 1

# ===========================================================================
# gh-pr-review/scripts/get-pr-info.sh: コメント URL の解析
# ===========================================================================

it "get-pr-info: コメント URL から PR 番号とコメント ID を取り出し PR 情報に添える"
make_fake_gh '"pr view 42 -R octo/repo --json"*) echo "{\"number\":42,\"title\":\"t\"}" ;;'
out=$("$REVIEW_SCRIPTS/get-pr-info.sh" "https://github.com/octo/repo/pull/42#discussion_r123456")
assert_eq 0 $?
assert_eq 42 "$(jq -r .number <<<"$out")"
assert_eq 123456 "$(jq -r .comment_id <<<"$out")"

it "get-pr-info: PR の URL（コメントなし）は comment_id が null"
out=$("$REVIEW_SCRIPTS/get-pr-info.sh" "https://github.com/octo/repo/pull/42")
assert_eq null "$(jq -r .comment_id <<<"$out")"

# ===========================================================================
# gh-pr-review/scripts/get-review-comments.sh: 投稿者が bot か人間かを user_type で返す
# ===========================================================================
# resolve の可否（bot のスレッドは対応後に resolve、人間のスレッドは返信のみ）を
# SKILL.md で判断するため、GraphQL の author.__typename（Bot / User）をそのまま出す

BOT_THREAD='{"id":"T1","isResolved":false,"isOutdated":false,"path":"a.txt","line":3,"comments":{"pageInfo":{"hasNextPage":false},"nodes":[{"databaseId":11,"id":"C1","body":"fix this","author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"},"createdAt":"2026-09-01T00:00:00Z","url":"u1","replyTo":null}]}}'
HUMAN_THREAD='{"id":"T2","isResolved":false,"isOutdated":false,"path":"b.txt","line":5,"comments":{"pageInfo":{"hasNextPage":false},"nodes":[{"databaseId":22,"id":"C2","body":"why?","author":{"__typename":"User","login":"alice"},"createdAt":"2026-09-01T00:01:00Z","url":"u2","replyTo":null}]}}'

it "get-review-comments: 各コメントに user_type（Bot / User）を付ける"
make_fake_gh "\"repo view --json owner\"*) echo octo ;;
  \"repo view --json name\"*) echo repo ;;
  \"api graphql --paginate\"*) printf '%s\n' '$BOT_THREAD' '$HUMAN_THREAD' ;;"
out=$("$REVIEW_SCRIPTS/get-review-comments.sh" 1)
assert_eq 0 $?
assert_eq Bot "$(jq -r '.[] | select(.id == 11) | .user_type' <<<"$out")"
assert_eq User "$(jq -r '.[] | select(.id == 22) | .user_type' <<<"$out")"

# ===========================================================================
# gh-pr-review/scripts/resolve-thread.sh: 人間のスレッドは既定で resolve しない
# ===========================================================================
# スレッドを閉じるのはレビュアーの権限。bot（Copilot 等）のスレッドは対応後に resolve するが、
# 人間が起こしたスレッドは返信だけにして相手に委ねる。明示的に --allow-human を付けたときだけ resolve する

# fake_gh_threads <threads_json>: resolve-thread.sh が読む reviewThreads と mutation を偽装する
fake_gh_threads() {
  make_fake_gh "\"repo view --json owner\"*) echo octo ;;
  \"repo view --json name\"*) echo repo ;;
  \"api graphql -F owner=\"*) echo '$1' ;;
  \"api graphql -F id=\"*) echo 'resolved' ;;"
}
THREADS='{"pageInfo":{"hasNextPage":false},"nodes":[
  {"id":"T1","isResolved":false,"comments":{"nodes":[{"databaseId":11,"author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"}}]}},
  {"id":"T2","isResolved":false,"comments":{"nodes":[{"databaseId":22,"author":{"__typename":"User","login":"alice"}},{"databaseId":23,"author":{"__typename":"User","login":"me"}}]}}
]}'

it "resolve-thread: bot が起こしたスレッドは resolve する"
fake_gh_threads "$THREADS"
out=$("$REVIEW_SCRIPTS/resolve-thread.sh" 1 11)
assert_eq 0 $?
assert_contains "$(fake_log gh)" "-F id=T1"

it "resolve-thread: 人間が起こしたスレッドは既定では resolve せず exit 1（mutation を呼ばない）"
fake_gh_threads "$THREADS"
err=$("$REVIEW_SCRIPTS/resolve-thread.sh" 1 23 2>&1)
assert_eq 1 $?
assert_contains "$err" "--allow-human"
assert_not_contains "$(fake_log gh)" "-F id="

it "resolve-thread: --allow-human を付ければ人間のスレッドも resolve する"
fake_gh_threads "$THREADS"
out=$("$REVIEW_SCRIPTS/resolve-thread.sh" 1 22 --allow-human)
assert_eq 0 $?
assert_contains "$(fake_log gh)" "-F id=T2"

# ===========================================================================
# gh-pr-review/scripts/request-rereview.sh: 再レビューはレビュー要求 API で依頼する
# ===========================================================================
# @copilot メンションコメントに応答するのは copilot-swe-agent（コーディング
# エージェント）で、copilot-pull-request-reviewer のレビューは提出されない。
# 依頼は requested_reviewers API に一本化し、要求が登録されたことを timeline の
# review_requested イベントで確かめてから待機する

# fake_gh_rereview: POST 後にだけ review_requested が現れる gh。
# 引数に "nogrow" を渡すと POST しても timeline が増えない状況を作る
fake_gh_rereview() {
  local grow="${1:-grow}"
  rm -f "$TEST_ROOT/requested"
  local event="if [ -f \"$TEST_ROOT/requested\" ]; then echo 2026-09-08T00:00:00Z; fi"
  [[ "$grow" == "nogrow" ]] && event=":"
  jq -n '{reviews: [{author: {login: "copilot-pull-request-reviewer"}, state: "COMMENTED", submittedAt: "2026-09-08T00:05:00Z"}]}' \
    > "$TEST_ROOT/reviews.json"
  make_fake_gh "\"auth status\"*) ;;
  \"repo view --json nameWithOwner\"*) echo octo/repo ;;
  \"api --paginate repos/{owner}/{repo}/issues/1/timeline\"*) $event ;;
  \"api -X POST repos/octo/repo/pulls/1/requested_reviewers\"*) touch \"$TEST_ROOT/requested\"; echo '{}' ;;
  \"pr view 1 --json number\"*) echo '{\"number\":1}' ;;
  \"pr view 1 --json reviews\"*) cat \"$TEST_ROOT/reviews.json\" ;;"
}

it "request-rereview: requested_reviewers API で Copilot にレビューを要求する"
fake_gh_rereview
out=$("$REVIEW_SCRIPTS/request-rereview.sh" 1 2>&1)
assert_eq 0 $?
assert_contains "$(fake_log gh)" "api -X POST repos/octo/repo/pulls/1/requested_reviewers"
assert_contains "$(fake_log gh)" "reviewers[]=copilot-pull-request-reviewer[bot]"

it "request-rereview: @copilot メンションコメントは投稿しない（応答するのは swe-agent でレビューは走らない）"
assert_not_contains "$(fake_log gh)" "issues/1/comments"

it "request-rereview: 登録された review_requested の時刻を基準に待機する"
assert_contains "$out" "SINCE: 2026-09-08T00:00:00Z"
assert_contains "$out" "新しいレビューが到着"

it "request-rereview: review_requested が増えなければ待機せずエラーで止まる"
fake_gh_rereview nogrow
err=$("$REVIEW_SCRIPTS/request-rereview.sh" 1 2>&1)
assert_eq 1 $?
assert_contains "$err" "ERROR"
assert_not_contains "$(fake_log gh)" "pr view 1 --json number"

it "request-rereview: レビュー要求 API が失敗したらフォールバックせずエラーで止まる"
make_fake_gh "\"auth status\"*) ;;
  \"repo view --json nameWithOwner\"*) echo octo/repo ;;
  \"api --paginate repos/{owner}/{repo}/issues/1/timeline\"*) ;;
  \"api -X POST repos/octo/repo/pulls/1/requested_reviewers\"*) echo 'gh: HTTP 404' >&2; exit 1 ;;"
err=$("$REVIEW_SCRIPTS/request-rereview.sh" 1 2>&1)
assert_eq 1 $?
assert_not_contains "$(fake_log gh)" "issues/1/comments"
assert_not_contains "$(fake_log gh)" "pr view 1 --json number"

# ===========================================================================
# gh-pr-review/scripts/decide-next.sh: 周回と応答はレビュー要求とレビュー提出で数える
# ===========================================================================
# Copilot のコメント（swe-agent の「対応を確認しました」等）はレビューではないので
# 周回を終わらせない。数えるのは Copilot へのレビュー要求とレビュー提出だけにする

# 未解決／解決済みのレビュースレッド（GraphQL reviewThreads の 1 ノード）
OPEN_THREAD='{"id":"T1","isResolved":false,"isOutdated":false,"path":"a.txt","line":3,"comments":{"pageInfo":{"hasNextPage":false},"nodes":[{"databaseId":11,"id":"C1","body":"fix this","author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"},"createdAt":"2026-09-08T02:30:00Z","url":"u1","replyTo":null}]}}'
OPEN_THREAD_WITH_REPLY='{"id":"T1","isResolved":false,"isOutdated":false,"path":"a.txt","line":3,"comments":{"pageInfo":{"hasNextPage":false},"nodes":[{"databaseId":11,"id":"C1","body":"fix this","author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"},"createdAt":"2026-09-08T02:30:00Z","url":"u1","replyTo":null},{"databaseId":12,"id":"C2","body":"Fixed in abc","author":{"__typename":"User","login":"me"},"createdAt":"2026-09-08T02:40:00Z","url":"u2","replyTo":{"databaseId":11}}]}}'
DONE_THREAD='{"id":"T1","isResolved":true,"isOutdated":false,"path":"a.txt","line":3,"comments":{"pageInfo":{"hasNextPage":false},"nodes":[{"databaseId":11,"id":"C1","body":"fix this","author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"},"createdAt":"2026-09-08T02:30:00Z","url":"u1","replyTo":null}]}}'

# fake_gh_decide <review_submitted_at> [head_sha] [reviewed_sha] [thread] [files]
# レビュー要求2件・レビュー1件（インライン指摘2件）の PR を模す。
# 既定では head とレビュー対象が一致し、スレッドは解決済み。
# files は PR の変更ファイルを出力するコマンド（既定はコードのファイル 1 件）。
# gh pr view --json files は先頭 100 件で切れるので、ページングする REST API で取る
fake_gh_decide() {
  local head="${2:-abc1234}" reviewed="${3:-abc1234}" thread="${4:-$DONE_THREAD}"
  local files="${5:-echo a.txt}"
  make_fake_gh "\"repo view --json nameWithOwner\"*) echo octo/repo ;;
  \"api --paginate repos/octo/repo/pulls/1/files\"*) $files ;;
  \"repo view --json owner\"*) echo octo ;;
  \"repo view --json name\"*) echo repo ;;
  \"pr view 1 --json headRefOid\"*) echo $head ;;
  \"api --paginate repos/{owner}/{repo}/issues/1/timeline\"*) printf '%s\n' 2026-09-08T00:00:00Z 2026-09-08T02:00:00Z ;;
  \"api --paginate repos/octo/repo/pulls/1/reviews?per_page=100\"*) echo '{\"id\":9,\"state\":\"COMMENTED\",\"body\":\"### 🟡 Changes recommended\"}' ;;
  \"api --paginate repos/octo/repo/pulls/1/reviews/9/comments?per_page=100\"*) printf '%s\n' 101 102 ;;
  \"api repos/octo/repo/pulls/1/reviews/9\"*) echo \"$1 $reviewed\" ;;
  \"api graphql --paginate\"*) echo '$thread' ;;"
}

it "decide-next: ROUND は Copilot へのレビュー要求の件数で数える"
fake_gh_decide 2026-09-08T03:00:00Z
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_eq 0 $?
assert_contains "$out" "ROUND: 2"
assert_contains "$out" "RESPONSE: review"
assert_contains "$out" "INLINE_COMMENTS: 2"

it "decide-next: 未解決スレッドがあれば ACT（対応する）"
fake_gh_decide 2026-09-08T03:00:00Z abc1234 abc1234 "$OPEN_THREAD"
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "UNRESOLVED: 1"
assert_contains "$out" "VERDICT: ACT"

it "decide-next: UNRESOLVED はスレッド数（返信を数えない）"
# get-review-comments.sh はスレッド内のコメントをフラットに返すので、
# 素朴に数えると返信の分だけ多くなる
fake_gh_decide 2026-09-08T03:00:00Z abc1234 abc1234 "$OPEN_THREAD_WITH_REPLY"
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "UNRESOLVED: 1"

it "decide-next: 未解決が残っていれば、push 済みでもまず ACT（対応が先）"
# 一部だけ直して push した状態。未対応のまま再レビューを要求しない
fake_gh_decide 2026-09-08T03:00:00Z def5678 abc1234 "$OPEN_THREAD"
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "HEAD_REVIEWED: no"
assert_contains "$out" "UNRESOLVED: 1"
assert_contains "$out" "VERDICT: ACT"

it "decide-next: 対応を push したのに要求していなければ REREVIEW_NEEDED（レビューのし忘れ）"
fake_gh_decide 2026-09-08T03:00:00Z def5678 abc1234
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "HEAD_REVIEWED: no"
assert_contains "$out" "VERDICT: REREVIEW_NEEDED"

it "decide-next: 計画（docs/plans/）だけの PR は、対応を push しても再レビューを要求せず STOP_PLAN_REVIEWED"
# 文章の計画は細部をいくらでも掘れるので、最初のレビューで方針の指摘を受けたら周回を止める
# （hook の pre-merge-check も同じ条件で再レビューを求めない）
fake_gh_decide 2026-09-08T03:00:00Z def5678 abc1234 "$DONE_THREAD" 'printf "%s\n" docs/plans/010_x.md docs/plans/sub/011_y.md'
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "HEAD_REVIEWED: no"
assert_contains "$out" "VERDICT: STOP_PLAN_REVIEWED"

it "decide-next: 計画だけの PR でも、未解決スレッドが残っていればまず ACT"
fake_gh_decide 2026-09-08T03:00:00Z def5678 abc1234 "$OPEN_THREAD" 'echo docs/plans/010_x.md'
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "VERDICT: ACT"

it "decide-next: 計画以外のファイルも含む PR は、これまでどおり REREVIEW_NEEDED"
fake_gh_decide 2026-09-08T03:00:00Z def5678 abc1234 "$DONE_THREAD" 'printf "%s\n" docs/plans/010_x.md src/docs/plans/x.md'
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "VERDICT: REREVIEW_NEEDED"

it "decide-next: 変更ファイルを取得できなければ、計画だけとみなさず REREVIEW_NEEDED"
fake_gh_decide 2026-09-08T03:00:00Z def5678 abc1234 "$DONE_THREAD" 'exit 1'
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "VERDICT: REREVIEW_NEEDED"

it "decide-next: 周回上限に達していれば要求せず STOP_LIMIT"
fake_gh_decide 2026-09-08T03:00:00Z def5678 abc1234
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1 --max-rounds 2)
assert_contains "$out" "VERDICT: STOP_LIMIT"

it "decide-next: 指摘に対応済み（未解決ゼロ・head レビュー済み）なら STOP_DECLINED"
# 全件 decline のように push を伴わない対応で終わった周。再度要求しても同じ
# レビューが返るだけなので、依頼せず終了する
fake_gh_decide 2026-09-08T03:00:00Z
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "HEAD_REVIEWED: yes"
assert_contains "$out" "UNRESOLVED: 0"
assert_contains "$out" "VERDICT: STOP_DECLINED"

it "decide-next: 要求後にレビューが来ていなければ WAITING（Copilot のコメントは応答に数えない）"
fake_gh_decide 2026-09-08T01:00:00Z
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "RESPONSE: none"
assert_contains "$out" "VERDICT: WAITING"
assert_not_contains "$out" "COMMENT_ONLY"
assert_not_contains "$(fake_log gh)" "issues/1/comments"

# ===========================================================================
# gh-wait-review.sh: 待つのはレビュー提出だけ
# ===========================================================================
# Copilot のコメント（swe-agent の応答）で待機を打ち切ると、レビューが来ていないのに
# 「応答あり」になる。待機間隔は GH_WAIT_INTERVALS で上書きできる（テスト用）

REPO="$TEST_ROOT/wait/app"
make_repo "$REPO"
make_remote "$REPO" github
cd "$REPO" || exit 1

# fake_gh_wait <copilot_review_at> [review_requested_at] [failed_check_runs]
# copilot_review_at に提出された Copilot のレビューが 1 件ある PR を模す。
# review_requested_at に "" を渡すと「レビュー要求なし」の PR を模す。
# failed_check_runs は head の Copilot チェックのうち失敗した件数（既定 0）
# レビュー一覧は gh が返す JSON の形で渡し、絞り込みはスクリプト側の jq に任せる
fake_gh_wait() {
  local requested="${2-2026-09-08T00:00:00Z}"
  local failed_runs="${3:-0}"
  local timeline="echo $requested"
  [[ -z "$requested" ]] && timeline=":"
  jq -n --arg at "$1" \
    '{reviews: [{author: {login: "copilot-pull-request-reviewer"}, state: "COMMENTED", submittedAt: $at}]}' \
    > "$TEST_ROOT/reviews.json"
  make_fake_gh "\"auth status\"*) ;;
  \"pr view 1 --json number\"*) echo '{\"number\":1}' ;;
  \"pr view 1 --json reviews\"*) cat \"$TEST_ROOT/reviews.json\" ;;
  \"pr view 1 --json headRefOid\"*) echo abc ;;
  \"api --paginate repos/{owner}/{repo}/commits/abc/check-runs?per_page=100\"*) seq 0 $failed_runs | tail -n +2 ;;
  \"api --paginate repos/{owner}/{repo}/issues/1/timeline\"*) $timeline ;;"
}

# add_review <login> <submitted_at>: fake_gh_wait のレビュー一覧にレビューを 1 件足す
add_review() {
  jq --arg login "$1" --arg at "$2" \
    '.reviews += [{author: {login: $login}, state: "COMMENTED", submittedAt: $at}]' \
    "$TEST_ROOT/reviews.json" > "$TEST_ROOT/reviews.json.new"
  mv "$TEST_ROOT/reviews.json.new" "$TEST_ROOT/reviews.json"
}

# add_review_without_author <submitted_at>: author が null のレビューを足す
# （投稿者のアカウントが削除されたレビューなど）
add_review_without_author() {
  jq --arg at "$1" '.reviews += [{author: null, state: "COMMENTED", submittedAt: $at}]' \
    "$TEST_ROOT/reviews.json" > "$TEST_ROOT/reviews.json.new"
  mv "$TEST_ROOT/reviews.json.new" "$TEST_ROOT/reviews.json"
}

it "gh-wait-review: 基準時刻より新しいレビュー提出で成功する"
fake_gh_wait 2026-09-08T01:00:00Z
out=$("$SCRIPTS_DIR/gh-wait-review.sh" 1 --since 2026-09-08T00:00:00Z)
assert_eq 0 $?
assert_contains "$out" "新しいレビューが到着"

it "gh-wait-review: --since 省略時は最後のレビュー要求以降のレビューを待つ"
# 要求(02:00)より後のレビュー(03:00)が既にあるので、待たずに成功する。
# これにより request-rereview.sh のあとに続けて呼んでも無害になる
fake_gh_wait 2026-09-08T03:00:00Z 2026-09-08T02:00:00Z
out=$(GH_WAIT_INTERVALS=0 "$SCRIPTS_DIR/gh-wait-review.sh" 1)
assert_eq 0 $?
assert_contains "$out" "新しいレビューが到着"

it "gh-wait-review: レビュー要求が無ければ待たずにエラーで終わる"
# 要求していないレビューは来ない。待つ意味がないので即座に理由を出して止まる
fake_gh_wait 2026-09-08T03:00:00Z ""
out=$(GH_WAIT_INTERVALS=0 "$SCRIPTS_DIR/gh-wait-review.sh" 1 2>&1)
assert_eq 6 $?
assert_contains "$out" "レビュー要求"
assert_not_contains "$out" "TIMEOUT"
# 案内はこのスクリプトと同じツリーの request-rereview.sh を指す（デプロイ先の旧版を案内しない）
assert_contains "$out" "$CLAUDE_DIR/skills/gh-pr-review/scripts/request-rereview.sh"

it "gh-wait-review: 要求後にまだレビューが無ければ待つ"
fake_gh_wait 2026-09-08T01:00:00Z 2026-09-08T02:00:00Z
out=$(GH_WAIT_INTERVALS=0 timeout 20 "$SCRIPTS_DIR/gh-wait-review.sh" 1)
assert_eq 1 $?
assert_contains "$out" "TIMEOUT"

it "gh-wait-review: レビューの実行が失敗していたら待たずに終わる（トークン切れ等）"
# 要求は登録されているがレビューが走らなかった場合、10分待っても来ない
fake_gh_wait 2026-09-08T01:00:00Z 2026-09-08T02:00:00Z 1
out=$(GH_WAIT_INTERVALS=0 timeout 20 "$SCRIPTS_DIR/gh-wait-review.sh" 1 2>&1)
assert_eq 7 $?
assert_contains "$out" "レビューの実行が失敗"
assert_not_contains "$out" "TIMEOUT"

it "gh-wait-review: 待機のたびに head を取り直す（待機中の push で古い head を見ない）"
# 起動時の 1 回だけで固定すると、待機中に push されても古い head の失敗を見て
# 打ち切ってしまう
fake_gh_wait 2026-09-08T01:00:00Z 2026-09-08T02:00:00Z 0
out=$(GH_WAIT_INTERVALS="0 0" timeout 20 "$SCRIPTS_DIR/gh-wait-review.sh" 1 2>&1)
assert_eq 1 $?
head_lookups=$(grep -c 'json headRefOid' "$TEST_ROOT/gh.log")
assert_eq yes "$( ((head_lookups >= 2)) && echo yes || echo "no($head_lookups)" )"

it "gh-wait-review: 新しいレビューが無ければタイムアウトする（Copilot のコメントは見ない）"
fake_gh_wait 2026-09-08T00:00:00Z
out=$(GH_WAIT_INTERVALS=0 timeout 20 "$SCRIPTS_DIR/gh-wait-review.sh" 1 --since 2026-09-08T00:00:00Z)
assert_eq 1 $?
assert_contains "$out" "TIMEOUT"
assert_not_contains "$(fake_log gh)" "issues/1/comments"

it "gh-wait-review: PR 作成者自身のレビューでは打ち切らない"
# インラインコメントに返信すると、作成者名義の COMMENTED レビューが作られる。
# 待っているのは要求した Copilot のレビューなので、これを到着とみなしてはいけない
# （2026-09-23、PR #201 で返信直後に「到着」と誤判定した）
fake_gh_wait 2026-09-08T01:00:00Z 2026-09-08T02:00:00Z
add_review me 2026-09-08T03:00:00Z
out=$(GH_WAIT_INTERVALS=0 timeout 20 "$SCRIPTS_DIR/gh-wait-review.sh" 1)
assert_eq 1 $?
assert_contains "$out" "TIMEOUT"

it "gh-wait-review: 作成者のレビューが後にあっても Copilot のレビューの時刻で報告する"
fake_gh_wait 2026-09-08T03:00:00Z 2026-09-08T02:00:00Z
add_review me 2026-09-08T04:00:00Z
out=$(GH_WAIT_INTERVALS=0 timeout 20 "$SCRIPTS_DIR/gh-wait-review.sh" 1)
assert_eq 0 $?
assert_contains "$out" "新しいレビューが到着しました（2026-09-08T03:00:00Z"

it "gh-wait-review: author が null のレビューがあっても Copilot のレビューを拾う"
# 投稿者が削除されたレビューは author が null になる。login をそのまま
# ascii_downcase に渡すと jq ごと失敗し、同じ一覧にある Copilot のレビューまで
# 捨てて時間切れになる
fake_gh_wait 2026-09-08T03:00:00Z 2026-09-08T02:00:00Z
add_review_without_author 2026-09-08T01:00:00Z
out=$(GH_WAIT_INTERVALS=0 timeout 20 "$SCRIPTS_DIR/gh-wait-review.sh" 1)
assert_eq 0 $?
assert_contains "$out" "新しいレビューが到着しました（2026-09-08T03:00:00Z"

# ===========================================================================
# gh-pr-review/scripts/get-latest-review.sh: レビュー失敗を指摘ゼロと区別する
# ===========================================================================
# Copilot はレビューできなかったときも本文だけのレビューを提出する
# （"Copilot wasn't able to review any files in this pull request."）。
# インライン指摘 0 件なので、区別しないと「指摘なし」と同じ扱いでマージへ進む

# fake_gh_review_body <body> [inline_ids...]
# 本文はアポストロフィを含むため、偽 gh のソースに埋め込まず JSON ファイルから読ませる
fake_gh_review_body() {
  local body="$1"; shift
  local ids="printf '%s\n' $*"
  [[ $# -eq 0 ]] && ids=":"
  jq -n --arg body "$body" '{id: 9, state: "COMMENTED", body: $body}' > "$TEST_ROOT/review.json"
  make_fake_gh "\"repo view --json nameWithOwner\"*) echo octo/repo ;;
  \"repo view --json owner\"*) echo octo ;;
  \"repo view --json name\"*) echo repo ;;
  \"pr view 1 --json headRefOid\"*) echo abc1234 ;;
  \"api --paginate repos/octo/repo/pulls/1/reviews?per_page=100\"*) cat \"$TEST_ROOT/review.json\" ;;
  \"api --paginate repos/octo/repo/pulls/1/reviews/9/comments?per_page=100\"*) $ids ;;
  \"api repos/octo/repo/pulls/1/reviews/9\"*) echo 2026-09-08T03:00:00Z abc1234 ;;
  \"api graphql --paginate\"*) echo '$DONE_THREAD' ;;
  \"api --paginate repos/{owner}/{repo}/issues/1/timeline\"*) echo 2026-09-08T02:00:00Z ;;"
}

it "get-latest-review: レビューできなかったレビューは REVIEW_FAILED: yes"
fake_gh_review_body "Copilot wasn't able to review any files in this pull request."
out=$("$REVIEW_SCRIPTS/get-latest-review.sh" 1)
assert_eq 0 $?
assert_contains "$out" "REVIEW_FAILED: yes"

it "get-latest-review: 通常のレビューは REVIEW_FAILED: no"
fake_gh_review_body "### 🟡 Changes recommended" 101 102
out=$("$REVIEW_SCRIPTS/get-latest-review.sh" 1)
assert_contains "$out" "REVIEW_FAILED: no"
assert_contains "$out" "INLINE_COMMENTS: 2"

it "decide-next: レビュー失敗は指摘なし（STOP_CLEAN）と区別する"
fake_gh_review_body "Copilot encountered an error and was unable to review this pull request."
out=$("$REVIEW_SCRIPTS/decide-next.sh" 1)
assert_contains "$out" "VERDICT: REVIEW_FAILED"
assert_not_contains "$out" "STOP_CLEAN"

# ===========================================================================
# レビュー要求の取得失敗は「要求なし」と区別する
# ===========================================================================
# 取得できないまま空として続けると、要求が登録されているのに「登録されません
# でした」と誤診したり、周回数を過少に見積もったりする。理由を出して止める

# fake_gh_timeline_fails: レビュー要求（timeline）の取得だけが失敗する gh
fake_gh_timeline_fails() {
  make_fake_gh "\"auth status\"*) ;;
  \"repo view --json nameWithOwner\"*) echo octo/repo ;;
  \"api --paginate repos/{owner}/{repo}/issues/1/timeline\"*) echo 'gh: HTTP 502' >&2; exit 1 ;;"
}

it "request-rereview: レビュー要求を取得できなければ理由を出して止まり、要求もしない"
fake_gh_timeline_fails
err=$("$REVIEW_SCRIPTS/request-rereview.sh" 1 2>&1)
assert_eq 1 $?
assert_contains "$err" "レビュー要求"
assert_not_contains "$(fake_log gh)" "requested_reviewers"

it "decide-next: レビュー要求を取得できなければ周回数を推測せず止まる"
fake_gh_timeline_fails
err=$("$REVIEW_SCRIPTS/decide-next.sh" 1 2>&1)
assert_eq 1 $?
assert_contains "$err" "レビュー要求"
assert_not_contains "$err" "ROUND:"

finish
