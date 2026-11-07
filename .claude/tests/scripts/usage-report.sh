#!/usr/bin/env bash
# .claude/scripts/usage-report.py（会話の記録から区間の費用を区分ごとに出す）のテスト
#
# 実行: bash .claude/tests/scripts/usage-report.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

REPORT="$SCRIPTS_DIR/usage-report.py"

PROJECT="$HOME/.claude/projects/-home-u-repo"
SESSION=11111111-2222-3333-4444-555555555555
MAIN="$PROJECT/$SESSION.jsonl"
SUB="$PROJECT/$SESSION/subagents"
mkdir -p "$SUB"

# user_text <text>: 依頼文やユーザーの発言の行
user_text() {
  jq -cn --arg t "$1" '{type: "user", message: {role: "user", content: $t}}'
}

# reply <id> <timestamp> <model> <input> <write_5m> <write_1h> <read> <output>: 応答の 1 行
reply() {
  jq -cn --arg id "$1" --arg ts "$2" --arg model "$3" \
    --argjson i "$4" --argjson w5 "$5" --argjson w1 "$6" --argjson r "$7" --argjson o "$8" '
    {type: "assistant", timestamp: $ts,
     message: {id: $id, model: $model, role: "assistant", content: [{type: "text", text: "x"}],
       usage: {input_tokens: $i, cache_creation_input_tokens: ($w5 + $w1), cache_read_input_tokens: $r,
         output_tokens: $o, cache_creation: {ephemeral_5m_input_tokens: $w5, ephemeral_1h_input_tokens: $w1},
         service_tier: "standard", inference_geo: "global", speed: "standard"}}}'
}

# 親（Fable 5.1: 入力 10、5 分の書き込み 12.5、1 時間の書き込み 20、読み出し 0.25、出力 50 ドル / 100 万トークン）
{
  user_text "始める"
  # 1 回の応答が本文と tool_use の 2 行に記録される。2 行目は数えない
  reply p1 2026-11-07T01:00:00.000Z claude-fable-5-1 1000 0 300000 0 200
  reply p1 2026-11-07T01:00:00.000Z claude-fable-5-1 1000 0 300000 0 200
  # JST の 11-06 23:59:59（区間の外）
  reply p2 2026-11-06T14:59:59.000Z claude-fable-5-1 1000000 0 0 0 0
  # JST の 11-07 00:00:00（区間の端。含む）
  reply p3 2026-11-06T15:00:00.000Z claude-fable-5-1 0 0 0 1000000 0
  # API の失敗で Claude Code が書く応答（使用量が 0）
  jq -cn '{type: "assistant", timestamp: "2026-11-07T02:00:00.000Z",
    message: {id: "s1", model: "<synthetic>", role: "assistant", content: [],
      usage: {input_tokens: 0, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: 0,
        cache_creation: {ephemeral_5m_input_tokens: 0, ephemeral_1h_input_tokens: 0}}}}'
} >"$MAIN"

# 担当（Opus 5.5: 入力 4、5 分 5、1 時間 8、読み出し 0.20、出力 20）
{
  user_text 'Review target: `/w/repo`'
  reply r1 2026-11-07T03:00:00.000Z claude-opus-5-5 100000 0 0 0 10000
} >"$SUB/agent-review.jsonl"
{
  user_text "最初に /retro を Skill で呼び、その手順に従って進める。"
  reply t1 2026-11-07T04:00:00.000Z claude-opus-5-5 0 400000 0 0 0
} >"$SUB/agent-retro.jsonl"
{
  # 依頼文の置き場の名前に retro を含む /delegate の依頼は担当に数える
  user_text "依頼文は /tmp/s/retro-eve/brief.md にある。最初に /delegate を呼び、その手順に従って進める。"
  reply d1 2026-11-07T05:00:00.000Z claude-opus-5-5 0 0 0 1000000 50000
} >"$SUB/agent-delegate.jsonl"
{
  user_text "調べる"
  # Haiku 5.5 は 10 万トークンを超えるプロンプトで単価が上がる（入力 0.10 → 0.50）
  reply h1 2026-11-07T06:00:00.000Z claude-haiku-5-5 200000 0 0 0 0
  reply h2 2026-11-07T06:01:00.000Z claude-haiku-5-5 100000 0 0 0 0
} >"$SUB/agent-explore.jsonl"
{
  user_text "前の日の担当"
  reply o1 2026-11-05T05:00:00.000Z claude-opus-5-5 1000000 0 0 0 0
} >"$SUB/agent-old.jsonl"

# マージの数を数えるリポジトリ（区間の中に 2 本、外に 1 本）
REPO="$TEST_ROOT/repo"
make_repo "$REPO"
# merge_at <branch> <date>: branch を作って 1 コミットし、main に --no-ff でマージする
merge_at() {
  git -C "$REPO" switch -q -c "$1"
  commit_file "$REPO" "$1.txt" "feat: $1"
  git -C "$REPO" switch -q main
  GIT_COMMITTER_DATE="$2" GIT_AUTHOR_DATE="$2" git -C "$REPO" merge -q --no-ff -m "Merge $1" "$1"
}
merge_at a "2026-11-06T23:00:00+09:00"
merge_at b "2026-11-07T10:00:00+09:00"
merge_at c "2026-11-07T20:00:00+09:00"

RANGE=(--since 2026-11-07T00:00+09:00 --until 2026-11-08T00:00+09:00)

it "usage-report: 区間の費用の合計を、モデルごとの価格でドルで出す"
out=$("$REPORT" "${RANGE[@]}" --repo "$REPO" "$MAIN" 2>&1)
assert_eq 0 $?
assert_contains "$out" "TRANSCRIPT: $MAIN"
assert_contains "$out" "TOTAL: \$10.18"

it "usage-report: 区間の端を JST で示す"
assert_contains "$out" "SINCE: 2026-11-07T00:00:00+09:00"
assert_contains "$out" "UNTIL: 2026-11-08T00:00:00+09:00"

it "usage-report: 親は 1 時間の書き込みを 2 倍の単価で数え、同じ message.id の 2 行目を数えない"
assert_contains "$out" "COST: 親 1 本 \$6.27 61.6% 書き直し 1 回 \$6.00"
assert_contains "$out" "TOKENS: 親 input=1000 write_5m=0 write_1h=300000 read=1000000 output=200"

it "usage-report: Review target: で始まる依頼はレビュー、/retro を呼ぶ依頼は振り返りに数える"
assert_contains "$out" "COST: レビュー 1 本 \$0.60 5.9% 書き直し 0 回 \$0.00"
assert_contains "$out" "COST: 振り返り 1 本 \$2.00 19.6% 書き直し 1 回 \$2.00"

it "usage-report: 依頼文のパスに retro を含むだけの依頼と、ほかのサブエージェントは担当に数え、区間に行の無い記録は数えない"
assert_contains "$out" "COST: 担当 2 本 \$1.31 12.9% 書き直し 0 回 \$0.00"
assert_contains "$out" "TOKENS: 担当 input=300000 write_5m=0 write_1h=0 read=1000000 output=50000"

it "usage-report: 文脈の書き直し（25 万トークンを超える書き込み）の回数と費用を出す"
assert_contains "$out" "REWRITES: 2 回 \$8.00"

it "usage-report: 区間の中のマージを --first-parent で数え、マージ 1 本あたりの費用を出す"
assert_contains "$out" "MERGES: 2 $REPO main"
assert_contains "$out" "PER_MERGE: \$5.09（マージ 2 本）"

it "usage-report: リモートの無いリポジトリは、worktree を渡しても主の worktree のブランチのマージを数える"
git -C "$REPO" worktree add -q -b feature "$TEST_ROOT/repo-feature" a
out=$("$REPORT" "${RANGE[@]}" --repo "$TEST_ROOT/repo-feature" "$MAIN" 2>&1)
assert_contains "$out" "MERGES: 2 $TEST_ROOT/repo-feature main"

it "usage-report: origin/HEAD のあるリポジトリは origin/HEAD のマージを数える"
make_remote "$REPO"
git -C "$REPO" remote set-head origin main
out=$("$REPORT" "${RANGE[@]}" --repo "$REPO" "$MAIN" 2>&1)
assert_contains "$out" "MERGES: 2 $REPO origin/HEAD"

it "usage-report: --repo が無ければマージ 1 本あたりの費用を出さない"
out=$("$REPORT" "${RANGE[@]}" "$MAIN" 2>&1)
assert_eq 0 $?
assert_contains "$out" "PER_MERGE: -（--repo なし）"

it "usage-report: 費用の大きいサブエージェントを依頼文の頭とともに並べる"
out=$("$REPORT" "${RANGE[@]}" "$MAIN" 2>&1)
assert_contains "$out" "TOP: \$2.00 振り返り 1 回"

it "usage-report: 区間を省くと JST の今日にする"
out=$("$REPORT" "$MAIN" 2>&1)
today=$(TZ=Asia/Tokyo date +%Y-%m-%d)
assert_contains "$out" "SINCE: ${today}T00:00:00+09:00"

it "usage-report: 記録を省くと CLAUDE_CODE_SESSION_ID の記録を探す"
out=$(CLAUDE_CODE_SESSION_ID=$SESSION "$REPORT" "${RANGE[@]}" 2>&1)
assert_eq 0 $?
assert_contains "$out" "TRANSCRIPT: $MAIN"

# 依頼文の書き方によらず、呼んだスキルと /code-review の印で区分を決める（別のセッション）
SESSION2=22222222-3333-4444-5555-666666666666
SUB2="$PROJECT/$SESSION2/subagents"
mkdir -p "$SUB2"
{
  user_text "始める"
  reply q1 2026-11-07T01:00:00.000Z claude-fable-5-1 0 0 0 0 0
} >"$PROJECT/$SESSION2.jsonl"
# skill_call <id> <timestamp> <skill>: Skill ツールでスキルを呼ぶ応答（使用量は出力 1000 トークン）
skill_call() {
  jq -cn --arg id "$1" --arg ts "$2" --arg s "$3" '
    {type: "assistant", timestamp: $ts,
     message: {id: $id, model: "claude-opus-5-5", role: "assistant",
       content: [{type: "tool_use", id: "u", name: "Skill", input: {skill: $s}}],
       usage: {input_tokens: 0, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: 1000,
         cache_creation: {ephemeral_5m_input_tokens: 0, ephemeral_1h_input_tokens: 0},
         service_tier: "standard", inference_geo: "global", speed: "standard"}}}'
}
{
  user_text '最初に Skill ツールで `retro` を呼び、その手順に従って進める。'
  skill_call k1 2026-11-07T02:00:00.000Z retro
} >"$SUB2/agent-retro-skill.jsonl"
{
  # /delegate を呼ぶ依頼文の後ろに /retro の語があっても担当
  user_text "最初に /delegate を呼び、その手順に従って進める。/retro の手順を直す。"
  skill_call k2 2026-11-07T02:00:00.000Z delegate
} >"$SUB2/agent-delegate-skill.jsonl"
{
  # 依頼文が Review target: で始まらない /code-review（meta の description で見分ける）
  user_text '`high effort → 8 inline angles`'
  reply k3 2026-11-07T02:00:00.000Z claude-opus-5-5 0 0 0 0 1000
} >"$SUB2/agent-old-review.jsonl"
echo '{"agentType":"general-purpose","description":"/code-review high /w/repo","name":"code-review"}' \
  >"$SUB2/agent-old-review.meta.json"

it "usage-report: 区分は、最初に呼んだスキル（retro か delegate）と /code-review の meta で決める"
out=$("$REPORT" "${RANGE[@]}" "$PROJECT/$SESSION2.jsonl" 2>&1)
assert_eq 0 $?
assert_contains "$out" "COST: 振り返り 1 本 \$0.02"
assert_contains "$out" "COST: 担当 1 本 \$0.02"
assert_contains "$out" "COST: レビュー 1 本 \$0.02"

it "usage-report: 時差の無い日時は失敗にする"
out=$("$REPORT" --since 2026-11-07T00:00 "$MAIN" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: --since は時差つきの ISO 8601"

it "usage-report: 価格表に無いモデルは失敗にする"
reply x1 2026-11-07T07:00:00.000Z claude-unknown-9 1 0 0 0 0 >>"$SUB/agent-explore.jsonl"
out=$("$REPORT" "${RANGE[@]}" "$MAIN" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: 価格表に無いモデル: claude-unknown-9"

finish
