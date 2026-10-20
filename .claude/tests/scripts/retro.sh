#!/usr/bin/env bash
# .claude/skills/retro/scripts/*.sh（振り返りの入力を集める）のテスト
#
# 実行: bash .claude/tests/scripts/retro.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

EVENTS="$CLAUDE_DIR/skills/retro/scripts/session-events.sh"

PROJECT="$HOME/.claude/projects/-home-u-repo"
SESSION=11111111-2222-3333-4444-555555555555
MAIN="$PROJECT/$SESSION.jsonl"
mkdir -p "$PROJECT/$SESSION/subagents"

# user_text <text>: ユーザーの発言の行
user_text() {
  jq -cn --arg t "$1" '{type: "user", message: {role: "user", content: $t}}'
}

# tool_error <content>: 失敗したツールの結果の行（content は文字列）
tool_error() {
  jq -cn --arg c "$1" \
    '{type: "user", message: {role: "user", content: [{type: "tool_result", tool_use_id: "t", is_error: true, content: $c}]}}'
}

# tool_ok <content>: 成功したツールの結果の行
tool_ok() {
  jq -cn --arg c "$1" \
    '{type: "user", message: {role: "user", content: [{type: "tool_result", tool_use_id: "t", content: $c}]}}'
}

{
  echo '{"type":"system","subtype":"turn_duration"}'
  user_text "PR を作ってください"
  tool_error "PreToolUse:Bash hook error: git add -A は禁止です
git add <file> でステージしてください"
  tool_error "PreToolUse:Bash hook error: git add -A は禁止です
git add <file> でステージしてください"
  tool_error "Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Git Destructive]."
  tool_error "Exit code 1
error: pathspec 'x' did not match"
  tool_error "<tool_use_error>File has not been read yet.</tool_use_error>"
  tool_ok "ok"
  user_text "<task-notification>
<task-id>b1</task-id>"
  jq -cn '{type: "user", isMeta: true, message: {role: "user", content: "スキルの本文"}}'
  user_text "[Request interrupted by user]"
  user_text "squash しないで
no-ff でマージしてください"
  jq -cn '{type: "assistant", message: {role: "assistant", content: [{type: "text", text: "はい"}]}}'
} > "$MAIN"

{
  user_text "親からの依頼文"
  tool_error "PreToolUse:Bash hook error: git add -A は禁止です
git add <file> でステージしてください"
  jq -cn '{type: "user", message: {role: "user", content: [{type: "tool_result", tool_use_id: "t", is_error: true,
    content: [{type: "text", text: "The user doesn'"'"'t want to proceed with this tool use. The tool use was rejected."}]}]}}'
} > "$PROJECT/$SESSION/subagents/agent-a1.jsonl"

it "session-events: 記録のパスと、隣のメモリのディレクトリと、読んだ記録の数を出す"
out=$("$EVENTS" "$MAIN")
assert_eq 0 $?
assert_contains "$out" "TRANSCRIPT: $MAIN"
assert_contains "$out" "MEMORY_DIR: $PROJECT/memory"
assert_contains "$out" "TRANSCRIPTS: 2"

it "session-events: hook の拒否を、サブエージェントの分も合わせて本文の 1 行目ごとに数える"
assert_contains "$out" "HOOK: 3 git add -A は禁止です"
assert_not_contains "$out" "hook error:"

it "session-events: 権限の拒否と、ユーザーが止めたツールを分けて数える"
assert_contains "$out" "PERMISSION: 1 Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Git Destructive]."
assert_contains "$out" "REJECTED: 1 The user doesn't want to proceed with this tool use."

it "session-events: そのほかの失敗は、終了コードと次の行、またはタグを除いた本文で数える"
assert_contains "$out" "TOOL: 1 Exit code 1: error: pathspec 'x' did not match"
assert_contains "$out" "TOOL: 1 File has not been read yet."

it "session-events: 成功したツールの結果は数えない"
assert_not_contains "$out" "ok"

it "session-events: ユーザーの発言を本セッションの順に出し、改行を空白にする"
users=$(grep '^USER: ' <<<"$out")
assert_eq "USER: PR を作ってください
USER: [Request interrupted by user]
USER: squash しないで no-ff でマージしてください" "$users"

it "session-events: 通知、スキルの本文（isMeta）、サブエージェントへの依頼文はユーザーの発言に入れない"
assert_not_contains "$out" "task-notification"
assert_not_contains "$out" "スキルの本文"
assert_not_contains "$out" "親からの依頼文"

it "session-events: 引数が無ければ CLAUDE_CODE_SESSION_ID の記録を探す"
out=$(CLAUDE_CODE_SESSION_ID=$SESSION "$EVENTS")
assert_eq 0 $?
assert_contains "$out" "TRANSCRIPT: $MAIN"

it "session-events: 引数も CLAUDE_CODE_SESSION_ID も無ければ失敗する"
out=$(env -u CLAUDE_CODE_SESSION_ID "$EVENTS" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "session-events: 記録が見つからなければ失敗する"
out=$(CLAUDE_CODE_SESSION_ID=no-such-session "$EVENTS" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "session-events: type を持つ行が 1 つも無い記録は、形が変わったとみなして失敗する（0 件と出さない）"
echo '{"kind":"other"}' > "$TEST_ROOT/unknown.jsonl"
out=$("$EVENTS" "$TEST_ROOT/unknown.jsonl" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "session-events: 長い本文は 160 文字で切る"
long=$(printf 'あ%.0s' $(seq 300))
tool_error "$long" > "$TEST_ROOT/long.jsonl"
out=$("$EVENTS" "$TEST_ROOT/long.jsonl")
line=$(grep '^TOOL: ' <<<"$out")
assert_eq "TOOL: 1 $(printf 'あ%.0s' $(seq 160))" "$line"

MEMORY_INDEX="$CLAUDE_DIR/skills/retro/scripts/memory-index.sh"
MEM="$TEST_ROOT/memory"
mkdir -p "$MEM"
cat > "$MEM/MEMORY.md" <<'EOF'
- [好み A](feedback-a.md) — 説明
- [事情 B](project-b.md) — 説明
- [消えた C](project-c.md) — 説明
EOF
printf -- '---\nname: a\n---\n\n本文 [[project-b]]\n' > "$MEM/feedback-a.md"
printf -- '---\nname: b\n---\n\n本文 [[feedback-gone]] と [[feedback-a]]\n' > "$MEM/project-b.md"
printf -- '---\nname: d\n---\n\n本文\n' > "$MEM/project-d.md"

it "memory-index: 索引の項目数と、索引にあってファイルの無いもの、ファイルがあって索引に無いものを出す"
out=$("$MEMORY_INDEX" "$MEM")
assert_eq 0 $?
assert_contains "$out" "ENTRIES: 3"
assert_contains "$out" "MISSING: project-c.md"
assert_contains "$out" "UNINDEXED: project-d.md"
assert_not_contains "$out" "MISSING: feedback-a.md"
assert_not_contains "$out" "UNINDEXED: MEMORY.md"

it "memory-index: 本文の [[名前]] で、同じ名前のファイルが無いものを出す"
assert_contains "$out" "BROKEN_LINK: project-b.md -> feedback-gone"
assert_not_contains "$out" "-> feedback-a"
assert_not_contains "$out" "-> project-b"

it "memory-index: 食い違いが無ければ件数の行だけを出す"
rm "$MEM/project-d.md"
sed -i '/project-c/d' "$MEM/MEMORY.md"
printf -- '---\nname: b\n---\n\n本文 [[feedback-a]]\n' > "$MEM/project-b.md"
out=$("$MEMORY_INDEX" "$MEM")
assert_eq "ENTRIES: 2" "$out"

it "memory-index: MEMORY.md が無ければ失敗する"
out=$("$MEMORY_INDEX" "$TEST_ROOT/no-memory" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

finish
