#!/usr/bin/env bash
# session-events.sh — Claude Code の会話の記録から、振り返りの手がかり（拒否、失敗、ユーザーの発言）を集める
#
# Usage: session-events.sh [TRANSCRIPT]
#   TRANSCRIPT  セッションの記録（~/.claude/projects/<プロジェクト>/<セッション>.jsonl）。
#               省略時は CLAUDE_CODE_SESSION_ID の記録を ~/.claude/projects/*/ から探す
#
# 記録の隣の <セッション>/subagents/*.jsonl（サブエージェントの記録）も読む。
#
# 出力:
#   TRANSCRIPT: <path>
#   MEMORY_DIR: <記録と同じディレクトリの memory>
#   TRANSCRIPTS: <読んだ記録の数（本体 + サブエージェント）>
#   <KIND>: <回数> <本文の要約>   失敗したツールの結果を、種類と要約ごとに回数の多い順に
#     HOOK        hook の拒否（`<event>:<tool> hook error: ` を除いた 1 行目）
#     PERMISSION  権限の拒否（自動モードの分類器など）
#     REJECTED    ユーザーが止めたツール
#     TOOL        そのほか（`Exit code N` は次の行とつなぐ。`<tool_use_error>` のタグは除く）
#   USER: <発言>  本体の記録のユーザーの発言を順に（改行は空白に）。通知（`<` で始まる）、
#                 スキルの本文（isMeta）、サブエージェントへの依頼文は除く
# 要約は 160 文字、発言は 300 文字で切る。
#
# 記録の形は Claude Code の公開の仕様ではない（ADR-0037）。type を持つ行が 1 つも無ければ、
# 形が変わったとみなして exit 1（0 件と出さない）。

set -euo pipefail

if (($# > 1)); then
  echo "Usage: session-events.sh [TRANSCRIPT]" >&2
  exit 1
fi

if (($# == 1)); then
  transcript=$1
else
  if [[ -z "${CLAUDE_CODE_SESSION_ID:-}" ]]; then
    echo "ERROR: 記録のパスを渡すか、CLAUDE_CODE_SESSION_ID を設定してください" >&2
    exit 1
  fi
  shopt -s nullglob
  found=("$HOME"/.claude/projects/*/"$CLAUDE_CODE_SESSION_ID".jsonl)
  shopt -u nullglob
  if ((${#found[@]} != 1)); then
    echo "ERROR: セッション $CLAUDE_CODE_SESSION_ID の記録が ${#found[@]} 件見つかりました（1 件のはず）" >&2
    exit 1
  fi
  transcript=${found[0]}
fi

if [[ ! -r "$transcript" ]]; then
  echo "ERROR: 記録を読めません: $transcript" >&2
  exit 1
fi

typed=$(jq -n '[inputs | select(type == "object" and has("type"))] | length' "$transcript")
if ((typed == 0)); then
  echo "ERROR: type を持つ行がありません（記録の形が変わったかもしれません）: $transcript" >&2
  exit 1
fi

shopt -s nullglob
subagents=("${transcript%.jsonl}"/subagents/*.jsonl)
shopt -u nullglob

echo "TRANSCRIPT: $transcript"
echo "MEMORY_DIR: $(dirname "$transcript")/memory"
echo "TRANSCRIPTS: $((1 + ${#subagents[@]}))"

# 失敗したツールの結果を、種類と要約に分ける
# shellcheck disable=SC2016  # jq のプログラム
jq -r '
  def body: if type == "array" then map(.text? // "") | join("\n") else tostring end;
  def first_line: split("\n") | map(select(test("\\S"))) | (.[0] // "(本文なし)");
  def cut: .[0:160];
  select(.type == "user" and (.message.content | type) == "array")
  | .message.content[]
  | select(.type == "tool_result" and .is_error == true)
  | (.content | body) as $b
  | if ($b | test("^\\S+ hook error: ")) then
      "HOOK\t" + ($b | sub("^\\S+ hook error: "; "") | first_line | cut)
    elif ($b | startswith("Permission for this action was denied")) then
      "PERMISSION\t" + ($b | first_line | cut)
    elif ($b | startswith("The user doesn'"'"'t want to proceed")) then
      "REJECTED\t" + ($b | first_line | sub("(?<s>proceed with this tool use\\.).*"; "\(.s)") | cut)
    elif ($b | test("^Exit code [0-9]+\n")) then
      ($b | split("\n")) as $l
      | "TOOL\t" + ($l[0] + ": " + ($l[1:] | join("\n") | first_line) | cut)
    else
      "TOOL\t" + ($b | gsub("</?tool_use_error>"; "") | first_line | cut)
    end
' "$transcript" "${subagents[@]}" |
  sort | uniq -c |
  awk '
    {
      count = $1
      sub(/^ *[0-9]+ /, "")
      kind = substr($0, 1, index($0, "\t") - 1)
      text = substr($0, index($0, "\t") + 1)
      order = (kind == "HOOK") ? 1 : (kind == "PERMISSION") ? 2 : (kind == "REJECTED") ? 3 : 4
      printf "%d\t%09d\t%s: %d %s\n", order, 999999999 - count, kind, count, text
    }
  ' |
  sort -t $'\t' -k1,1n -k2,2n -k3,3 | cut -f3-

# ユーザーの発言（本体の記録だけ。サブエージェントの記録の発言は親からの依頼文）
jq -r '
  select(.type == "user" and (.isMeta // false) == false and (.isSidechain // false) == false)
  | .message.content
  | if type == "string" then . else map(select(.type == "text") | .text) | join("\n") end
  | select(test("\\S") and (startswith("<") | not))
  | "USER: " + (gsub("\\s*\n\\s*"; " ") | .[0:300])
' "$transcript"
