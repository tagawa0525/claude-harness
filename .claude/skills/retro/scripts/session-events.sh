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
#   TRANSCRIPTS: <読んだ記録の数（本体 + サブエージェント）>
#   <KIND>: <回数> <本文の要約>   失敗したツールの結果を、種類と要約ごとに回数の多い順に
#     HOOK        hook の拒否（`<event>:<tool> hook error: ` を除いた 1 行目）。同じ拒否をまとめるため、
#                 コミットの番号を `<hash>`、英数字と `-` `#` に続かない数を `N` に置き換える（ADR-0015 は残す）
#     PERMISSION  権限の拒否（1 行目が自動モードの分類器などの拒否）
#     REJECTED    ユーザーが止めたツール
#     TOOL        そのほか（`Exit code N` は、エラーらしい行の最後、無ければ最後の行とつなぐ。
#                 `<tool_use_error>` のタグは除く）
#   USER: <発言>  本体の記録のユーザーの発言を順に（改行は空白に）。ターンの途中で送った発言
#                 （attachment の queued_command）を含み、スラッシュコマンドは `/<名前> <引数>` にする。
#                 通知（`<` で始まる）、スキルの本文（isMeta）、要約（isCompactSummary）、
#                 サブエージェントへの依頼文は除く
# 要約は 160 文字、発言は 300 文字で切る。
#
# 記録の形は Claude Code の公開の仕様ではない（ADR-0037）。type を持つ行が 1 つも無いか、
# ツールを呼んだ行があるのに tool_result が 1 つも無ければ、形が変わったとみなして exit 1（0 件と出さない）。

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

shopt -s nullglob
subagents=("${transcript%.jsonl}"/subagents/*.jsonl)
shopt -u nullglob

# 形の検査: type を持つ行（本体）と、ツールの呼び出しと結果（本体とサブエージェント）を 1 度に数える
# shellcheck disable=SC2016  # jq のプログラム
read -r typed uses results < <(jq -rn '
  reduce (inputs | select(type == "object")) as $e ([0, 0, 0];
    ([$e.message?.content? | arrays | .[] | .type?]) as $t
    | .[0] += (if ($e | has("type")) and input_filename == $main then 1 else 0 end)
    | .[1] += ($t | map(select(. == "tool_use")) | length)
    | .[2] += ($t | map(select(. == "tool_result")) | length))
  | @tsv
' --arg main "$transcript" "$transcript" "${subagents[@]}")
if ((typed == 0)); then
  echo "ERROR: type を持つ行がありません（記録の形が変わったかもしれません）: $transcript" >&2
  exit 1
fi
if ((uses > 0 && results == 0)); then
  echo "ERROR: ツールの呼び出しが $uses 件あるのに tool_result がありません（記録の形が変わったかもしれません）: $transcript" >&2
  exit 1
fi

echo "TRANSCRIPT: $transcript"
echo "TRANSCRIPTS: $((1 + ${#subagents[@]}))"

# 失敗したツールの結果を、種類と要約に分ける
# shellcheck disable=SC2016  # jq のプログラム
jq -r '
  def body: if type == "array" then map(.text? // "") | join("\n") else tostring end;
  def first_line: split("\n") | map(select(test("\\S"))) | (.[0] // "(本文なし)");
  def cut: .[0:160];
  def norm:
    gsub("(?<![0-9A-Za-z])(?=[0-9a-f]*[0-9])[0-9a-f]{7,40}(?![0-9A-Za-z])"; "<hash>")
    | gsub("(?<![-#0-9A-Za-z])[0-9]+"; "N");
  # 終了コードのある失敗の出力は標準出力と標準エラーが混ざる。エラーらしい行の最後を選ぶ
  def error_line:
    map(select(test("\\S")))
    | (map(select(test("error|fatal|denied|cannot|not found|no such|failed(?!: 0)|できません|ありません|失敗"; "i")))
       | last) // last // "(本文なし)";
  select(.type == "user" and (.message.content | type) == "array")
  | .message.content[]
  | select(.type == "tool_result" and .is_error == true)
  | (.content | body) as $b
  | if ($b | test("^\\S+ hook error: ")) then
      "HOOK\t" + ($b | sub("^\\S+ hook error: "; "") | first_line | norm | cut)
    elif ($b | first_line | test("^Permission for this action was denied|auto mode classifier")) then
      "PERMISSION\t" + ($b | first_line | cut)
    elif ($b | startswith("The user doesn'"'"'t want to proceed")) then
      "REJECTED\t" + ($b | first_line | sub("(?<s>proceed with this tool use\\.).*"; "\(.s)") | cut)
    elif ($b | test("^Exit code [0-9]+\n")) then
      ($b | split("\n")) as $l
      | "TOOL\t" + ($l[0] + ": " + ($l[1:] | error_line) | cut)
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
# shellcheck disable=SC2016  # jq のプログラム
jq -r '
  def text_of: if type == "string" then . else map(select(.type == "text") | .text) | join("\n") end;
  def command:
    if test("<command-name>") then
      (capture("<command-name>(?<n>[^<]*)</command-name>").n) as $n
      | ((capture("<command-args>(?<a>.*?)</command-args>"; "s") | .a) // "") as $a
      | if $a == "" then $n else $n + " " + $a end
    else . end;
  (
    select(.type == "user" and (.isMeta // false) == false and (.isSidechain // false) == false
      and (.isCompactSummary // false) == false and (.isVisibleInTranscriptOnly // false) == false)
    | .message.content | text_of | command
  ),
  (
    select(.type == "attachment" and .attachment.type? == "queued_command"
      and (.attachment.commandMode // "prompt") == "prompt")
    | .attachment.prompt | text_of
  )
  | select(test("\\S") and (startswith("<") | not))
  | "USER: " + (gsub("\\s*\n\\s*"; " ") | .[0:300])
' "$transcript"
