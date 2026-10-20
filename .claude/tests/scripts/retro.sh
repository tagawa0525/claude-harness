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
  tool_error "PreToolUse:Bash hook error: 最後の push (e7c9939) は自動レビューを受けていません（3 件）"
  tool_error "PreToolUse:Bash hook error: 最後の push (71cfb82) は自動レビューを受けていません（12 件）"
  tool_error "The server-side auto mode classifier gave no verdict for Bash: rm -rf x"
  tool_error "PreToolUse:Bash hook error: ADR-0015 により止めます。コミットe7c9939dは約10分前"
  tool_error "Exit code 1
✗ fixture: missing 'auto mode classifier'"
  tool_error "Exit code 2
SKILL.md
grep: x: No such file or directory
SKILL.md"
  tool_error "Exit code 127
passed: 237, failed: 0
bash: command not found: shellcheck"
  tool_ok "成功の印"
  tool_error "Error: something happened in auto mode classifier test"
  tool_error "Permission to use Bash with command rm -rf x has been denied."
  tool_error "Permission for this command was denied by a built-in safety check."
  jq -cn '{type: "attachment", attachment: {type: "queued_command", prompt: "別のエージェントの言葉", commandMode: "prompt", origin: {kind: "agent"}}}'
  tool_error "The user doesn't want to proceed with this tool use. The tool use was rejected. To tell you how to proceed, the user said:
squash しないで
no-ff にして"
  jq -cn '{type: "user", message: {role: "user", content: [{type: "text", text: "<ide_selection>選んだ行</ide_selection>"}, {type: "text", text: "IDE から送った訂正"}]}}'
  jq -cn '{type: "attachment", attachment: {type: "queued_command", prompt: "途中で送った訂正", commandMode: "prompt", origin: {kind: "human"}}}'
  jq -cn '{type: "attachment", attachment: {type: "queued_command", prompt: "<task-notification>x</task-notification>", commandMode: "prompt"}}'
  jq -cn '{type: "user", isCompactSummary: true, isVisibleInTranscriptOnly: true, message: {role: "user", content: "This session is being continued"}}'
  user_text "<command-message>gh-pr-merge</command-message>
<command-name>/gh-pr-merge</command-name>
<command-args>371</command-args>"
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

it "session-events: 記録のパスと、読んだ記録の数を出す"
out=$("$EVENTS" "$MAIN")
assert_eq 0 $?
assert_contains "$out" "TRANSCRIPT: $MAIN"
assert_contains "$out" "TRANSCRIPTS: 2"

it "session-events: hook の拒否を、サブエージェントの分も合わせて本文の 1 行目ごとに数える"
assert_contains "$out" "HOOK: 3 git add -A は禁止です"
assert_not_contains "$out" "hook error:"

it "session-events: hook の拒否の本文のコミットの番号と数は置き換えて、同じ拒否をまとめて数える"
assert_contains "$out" "HOOK: 2 最後の push (<hash>) は自動レビューを受けていません（N 件）"

it "session-events: ADR などの記号に続く番号は残し、日本語に接したコミットの番号も置き換える"
assert_contains "$out" "HOOK: 1 ADR-0015 により止めます。コミット<hash>は約N分前"

it "session-events: 1 行目の途中に分類器の語があるだけの失敗は権限の拒否に数えない"
assert_contains "$out" "TOOL: 1 Error: something happened in auto mode classifier test"

it "session-events: 設定の deny による拒否も権限の拒否に数える"
assert_contains "$out" "PERMISSION: 1 Permission to use Bash with command rm -rf x has been denied."
assert_contains "$out" "PERMISSION: 1 Permission for this command was denied by a built-in safety check."

it "session-events: 人以外から届いた queued_command はユーザーの発言に入れない"
assert_not_contains "$out" "別のエージェントの言葉"

it "session-events: ユーザーがツールを止めて書いた言葉を残す"
assert_contains "$out" "REJECTED: 1 the user said: squash しないで no-ff にして"

it "session-events: 発言の中のタグだけのブロックを除き、残りの本文を発言として出す"
assert_contains "$out" "USER: IDE から送った訂正"
assert_not_contains "$out" "選んだ行"

it "session-events: 権限の拒否と、ユーザーが止めたツールを分けて数える"
assert_contains "$out" "PERMISSION: 1 Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Git Destructive]."
assert_contains "$out" "PERMISSION: 1 The server-side auto mode classifier gave no verdict for Bash: rm -rf x"
assert_contains "$out" "REJECTED: 1 The user doesn't want to proceed with this tool use."

it "session-events: そのほかの失敗は、終了コードと次の行、またはタグを除いた本文で数える"
assert_contains "$out" "TOOL: 1 Exit code 1: error: pathspec 'x' did not match"
assert_contains "$out" "TOOL: 1 File has not been read yet."

it "session-events: 終了コードのある失敗は、出力の最後の行（標準エラーが来る）をつなぐ"
assert_contains "$out" "TOOL: 1 Exit code 127: bash: command not found: shellcheck"

it "session-events: 終了コードのある失敗は、エラーらしい行があればそれを優先する"
assert_contains "$out" "TOOL: 1 Exit code 2: grep: x: No such file or directory"

it "session-events: 本文の途中に分類器の語があるだけの失敗は権限の拒否に数えない"
assert_contains "$out" "TOOL: 1 Exit code 1: ✗ fixture: missing 'auto mode classifier'"

it "session-events: 成功したツールの結果は数えない"
assert_not_contains "$out" "成功の印"

it "session-events: ユーザーの発言を本セッションの順に出し、改行を空白にする"
users=$(grep '^USER: ' <<<"$out")
assert_eq "USER: PR を作ってください
USER: IDE から送った訂正
USER: 途中で送った訂正
USER: /gh-pr-merge 371
USER: [Request interrupted by user]
USER: squash しないで no-ff でマージしてください" "$users"

it "session-events: 通知、スキルの本文（isMeta）、サブエージェントへの依頼文はユーザーの発言に入れない"
assert_not_contains "$out" "task-notification"
assert_not_contains "$out" "スキルの本文"
assert_not_contains "$out" "親からの依頼文"

it "session-events: 要約（compact）の本文はユーザーの発言に入れない"
assert_not_contains "$out" "This session is being continued"

it "session-events: ツールを呼んだのに結果の行が 1 つも無い記録は、形が変わったとみなして失敗する"
{
  jq -cn '{type: "assistant", message: {role: "assistant", content: [{type: "tool_use", id: "t", name: "Bash", input: {}}]}}'
  jq -cn '{type: "user", message: {role: "user", content: [{type: "result", tool_use_id: "t", content: "x"}]}}'
} > "$TEST_ROOT/renamed.jsonl"
out=$("$EVENTS" "$TEST_ROOT/renamed.jsonl" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

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

it "session-events: 書きかけの行（最後の行が途中で切れている）は飛ばして読む"
cp "$MAIN" "$TEST_ROOT/partial.jsonl"
printf '%s' '{"type":"assistant","message":{"content":[{"type":"tool_use"' >> "$TEST_ROOT/partial.jsonl"
out=$("$EVENTS" "$TEST_ROOT/partial.jsonl" 2>&1)
assert_eq 0 $?
assert_contains "$out" "HOOK: 2 git add -A は禁止です"

it "session-events: user 以外の行の tool_result では、形の検査を通さない"
{
  jq -cn '{type: "assistant", message: {role: "assistant", content: [{type: "tool_use", id: "t", name: "Bash", input: {}}]}}'
  jq -cn '{type: "api-request-blob", message: {content: [{type: "tool_result", tool_use_id: "t", content: "x"}]}}'
} > "$TEST_ROOT/blob.jsonl"
out=$("$EVENTS" "$TEST_ROOT/blob.jsonl" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "session-events: - で始まる相対パスの記録も読む"
mkdir -p "$TEST_ROOT/rel"
cp "$MAIN" "$TEST_ROOT/rel/-home-u.jsonl"
out=$(cd "$TEST_ROOT/rel" && "$EVENTS" -home-u.jsonl 2>&1)
assert_eq 0 $?
assert_contains "$out" "HOOK: 2 git add -A は禁止です"

it "session-events: 英字を含まない 7 桁以上の数はコミットの番号でなく数として置き換える"
tool_error "PreToolUse:Bash hook error: 大きさ 10485760 を超えています" > "$TEST_ROOT/big.jsonl"
out=$("$EVENTS" "$TEST_ROOT/big.jsonl")
assert_contains "$out" "HOOK: 1 大きさ N を超えています"

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
- [外部](https://example.com/README.md) — 外部の文書、[ADR](../docs/adr/0001.md) を参照
EOF
printf -- '---\nname: a\n---\n\n本文 [[project-b|別名]] と [[project-b#節]]\n\n`[[ -n "$x" ]]`\n\n```bash\n[[ -z "$y" ]]\n```\n' > "$MEM/feedback-a.md"
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

it "memory-index: 外部への索引のリンクは突き合わせない"
assert_not_contains "$out" "example.com"

it "memory-index: [[名前|別名]] と [[名前#節]] は名前で解決し、コードの中の [[ ]] は読まない"
assert_not_contains "$out" "BROKEN_LINK: feedback-a.md"

it "memory-index: 本文の [[名前]] で、同じ名前のファイルが無いものを出す"
assert_contains "$out" "BROKEN_LINK: project-b.md -> feedback-gone"
assert_not_contains "$out" "-> feedback-a"
assert_not_contains "$out" "-> project-b"

it "memory-index: [[名前]] がフロントマターの name に一致するファイルがあれば、参照先があるとみなす"
printf -- '---\nname: d-name\n---\n\n本文\n' > "$MEM/project-d.md"
printf -- '---\nname: e\n---\n\n本文 [[d-name]]\n' > "$MEM/project-e.md"
out=$("$MEMORY_INDEX" "$MEM")
assert_not_contains "$out" "-> d-name"
rm "$MEM/project-e.md"

it "memory-index: フロントマターの name の引用符を外し、フロントマターの無いファイルの本文の name: は読まない"
printf -- '---\nname: "d-quoted"\n---\n\n本文\n' > "$MEM/project-d.md"
printf -- '本文だけ\nname: bogus\n' > "$MEM/project-f.md"
printf -- '---\nname: e\n---\n\n本文 [[d-quoted]] [[bogus]]\n' > "$MEM/project-e.md"
out=$("$MEMORY_INDEX" "$MEM")
assert_not_contains "$out" "-> d-quoted"
assert_contains "$out" "BROKEN_LINK: project-e.md -> bogus"
rm "$MEM/project-e.md" "$MEM/project-f.md"

it "memory-index: 別のディレクトリへのリンクは索引の項目に数えない"
assert_not_contains "$out" "../docs"

it "memory-index: 名前の空の [[...]] で止まらず、~~~ のフェンスの中も読まない"
printf -- '---\nname: g\n---\n\n本文 [[#節]] [[ ]]\n\n~~~\n[[ -n x ]]\n~~~\n' > "$MEM/project-g.md"
out=$("$MEMORY_INDEX" "$MEM" 2>&1)
assert_eq 0 $?
assert_not_contains "$out" "BROKEN_LINK: project-g.md"
rm "$MEM/project-g.md"

it "memory-index: 索引の ./ で始まるリンク、節や題のついたリンクもファイル名で突き合わせる"
sed -i 's|(project-b.md)|(./project-b.md#why)|; s|(feedback-a.md)|(feedback-a.md "題")|' "$MEM/MEMORY.md"
out=$("$MEMORY_INDEX" "$MEM")
assert_not_contains "$out" "UNINDEXED: project-b.md"
assert_not_contains "$out" "MISSING: ./project-b.md"
assert_not_contains "$out" "UNINDEXED: feedback-a.md"

it "memory-index: 食い違いが無ければ件数の行だけを出す"
rm "$MEM/project-d.md"
sed -i '/project-c/d; /example.com/d' "$MEM/MEMORY.md"
printf -- '---\nname: b\n---\n\n本文 [[feedback-a]]\n' > "$MEM/project-b.md"
out=$("$MEMORY_INDEX" "$MEM")
assert_eq "ENTRIES: 2" "$out"

it "memory-index: MEMORY.md が無ければ失敗する"
out=$("$MEMORY_INDEX" "$TEST_ROOT/no-memory" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

finish
