#!/usr/bin/env bash
# .claude/scripts/quote-in-transcript.sh（依頼で括ったユーザーの言葉を会話の記録の発言で確かめる）のテスト
#
# 実行: bash .claude/tests/scripts/quote-in-transcript.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

QUOTE="$SCRIPTS_DIR/quote-in-transcript.sh"

LOG="$TEST_ROOT/session.jsonl"
{
  # ユーザーが打った発言（UTC の 18:06 は JST の翌日 03:06）
  jq -cn '{type: "user", origin: {kind: "human"}, timestamp: "2026-11-07T18:06:39.476Z",
    message: {role: "user", content: "基本的に推奨でいいのですが、promptよりhook/scriptに振ったほうがいいのでは？\n次の文。"}}'
  # 本文の配列の中の text と、<command-args> の中
  jq -cn '{type: "user", origin: {kind: "human"}, timestamp: "2026-11-07T19:00:00.000Z",
    message: {role: "user", content: [{type: "text", text: "<command-name>/x</command-name><command-args>引数の中の文。</command-args>"}]}}'
  # ターンの途中に送った発言
  jq -cn '{type: "attachment", timestamp: "2026-11-07T20:00:00.000Z",
    attachment: {type: "queued_command", prompt: "途中で送った文。"}}'
  # 貼り付けた文は発言に数えない
  jq -cn '{type: "user", origin: {kind: "human"}, timestamp: "2026-11-07T21:00:00.000Z",
    message: {role: "user", content: "前の文。<pasted_content id=\"1\">貼り付けた文。</pasted_content>"}}'
  # モデルの応答、origin の無い user の行（ツールの結果など）は読まない
  jq -cn '{type: "assistant", timestamp: "2026-11-07T22:00:00.000Z",
    message: {role: "assistant", content: [{type: "text", text: "応答の文。"}]}}'
  jq -cn '{type: "user", timestamp: "2026-11-07T23:00:00.000Z",
    message: {role: "user", content: "ツールの結果の文。"}}'
  # 書きかけの行
  printf '%s' '{"type": "user", "orig'
} >"$LOG"

# quote_file <text>: 引用の文を書いたファイルのパスを出す
quote_file() {
  local f
  f=$(mktemp "$TEST_ROOT/quote.XXXXXX")
  printf '%s\n' "$1" >"$f"
  echo "$f"
}

it "quote-in-transcript: 引用を含む発言の記録のパス、timestamp、JST の日時と全文を出し、0 で終わる"
out=$("$QUOTE" "$(quote_file '基本的に推奨でいいのですが、promptよりhook/scriptに振ったほうがいいのでは？')" "$LOG" 2>&1)
assert_eq 0 $?
assert_contains "$out" "$LOG 2026-11-07T18:06:39.476Z 2026-11-08 03:06 JST"
assert_contains "$out" "次の文。"

it "quote-in-transcript: 「…」で切った部分が同じ発言に同じ順で出れば見つける"
out=$("$QUOTE" "$(quote_file '基本的に推奨で…振ったほうがいいのでは？')" "$LOG" 2>&1)
assert_eq 0 $?
assert_contains "$out" "2026-11-08 03:06 JST"

it "quote-in-transcript: 部分の順が違えば見つけず、1 で終わる"
out=$("$QUOTE" "$(quote_file '振ったほうがいいのでは？…基本的に推奨で')" "$LOG" 2>&1)
assert_eq 1 $?
assert_not_contains "$out" "03:06 JST"

it "quote-in-transcript: <command-args> の中と、ターンの途中に送った発言を読む"
out=$("$QUOTE" "$(quote_file '引数の中の文。')" "$LOG" 2>&1)
assert_eq 0 $?
out=$("$QUOTE" "$(quote_file '途中で送った文。')" "$LOG" 2>&1)
assert_eq 0 $?

it "quote-in-transcript: 貼り付けた文、モデルの応答、origin の無い user の行は読まない"
for q in '貼り付けた文。' '応答の文。' 'ツールの結果の文。'; do
  out=$("$QUOTE" "$(quote_file "$q")" "$LOG" 2>&1)
  assert_eq 1 $?
done

it "quote-in-transcript: 空の引用は止める"
out=$("$QUOTE" "$(quote_file '…')" "$LOG" 2>&1)
assert_eq 2 $?
assert_contains "$out" "引用の文が空"

it "quote-in-transcript: 引数が足りなければ使い方を出して 2 で終わる"
out=$("$QUOTE" "$(quote_file 'x')" 2>&1)
assert_eq 2 $?
assert_contains "$out" "Usage"

it "quote-in-transcript: 読めない記録は 2 で終わる"
out=$("$QUOTE" "$(quote_file 'x')" "$TEST_ROOT/none.jsonl" 2>&1)
assert_eq 2 $?

it "quote-in-transcript: 使い方は記録の探し方の文の終わりまで出す"
out=$("$QUOTE" 2>&1)
assert_contains "$out" ".claude/projects/*/*.jsonl を渡す"

it "quote-in-transcript: - で始まる相対パスの記録も、jq のオプションでなく記録として読む"
mkdir -p "$TEST_ROOT/projects/-home-u-repo"
cp "$LOG" "$TEST_ROOT/projects/-home-u-repo/s.jsonl"
out=$(cd "$TEST_ROOT/projects" && "$QUOTE" "$(quote_file '記録に無い文。')" -home-u-repo/s.jsonl 2>&1)
assert_eq 1 $?
out=$(cd "$TEST_ROOT/projects" && "$QUOTE" "$(quote_file '途中で送った文。')" -home-u-repo/s.jsonl 2>&1)
assert_eq 0 $?
assert_contains "$out" "-home-u-repo/s.jsonl 2026-11-07T20:00:00.000Z"

it "quote-in-transcript: 空の引用は、空の記録に掛けても 2 で終わる"
: >"$TEST_ROOT/empty.jsonl"
out=$("$QUOTE" "$(quote_file '')" "$TEST_ROOT/empty.jsonl" 2>&1)
assert_eq 2 $?
assert_contains "$out" "引用の文が空"

finish
