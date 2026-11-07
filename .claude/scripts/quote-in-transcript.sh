#!/usr/bin/env bash
# quote-in-transcript.sh — 依頼で「」に括ったユーザーの言葉が、会話の記録のユーザーの発言にあるかを確かめる
#
# Usage: quote-in-transcript.sh <引用の文のファイル> <記録の .jsonl>...
#   引用の文のファイル  括る「」を除いた引用の文。省いたところと文の途中で切った端の「…」は含める（シェルの
#                       引用符と食い違わないよう、引数でなくファイルで渡す）
#   記録の .jsonl       ~/.claude/projects/<プロジェクト>/<セッション>.jsonl。どの記録か分からなければ
#                       ~/.claude/projects/*/*.jsonl を渡す
#
# 読むのはユーザーが打った発言だけ: type が user で origin.kind が human の行と、ターンの途中に送った
# queued_command の行。記録にはツールの結果、ほかの担当の報告、圧縮の要約、コマンドの出力、IDE の通知、
# 貼り付けた文も入るので、grep では確かめにならない。<command-args> の中は発言に含め、ほかの < で始まる
# 要素と <pasted_content> は除く。origin の無い user の行の発言は拾わない（その引用は要約に落ちる）。
#
# 出力: 引用を「…」で切った部分がどれも同じ順に出る発言ごとに
#   <記録のパス> <timestamp> <JST の日時>
#   <発言の全文>
#   ---
# 見つかっても、引用の両端が全文の文の区切り（「。」「？」「！」、改行、発言の端）か「…」の印に当たるかは
# 読む側が見る（文の途中で切って印の無い引用は、後半の条件や否定を落としうる）。
#
# 終了コード: 0 見つかった / 1 見つからない / 2 使い方の誤り、記録が読めない、引用の文が空
set -euo pipefail

if (($# < 2)); then
  sed -n '3,8p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
fi
quote=$1
shift
if [[ ! -r "$quote" ]]; then
  echo "ERROR: 引用の文のファイルが読めない: $quote" >&2
  exit 2
fi
for log in "$@"; do
  if [[ ! -r "$log" ]]; then
    echo "ERROR: 記録が読めない: $log" >&2
    exit 2
  fi
done

# 空の引用は、記録の行が無くても止める（下の jq は記録の行ごとに確かめるので、空の記録では止まらない）
if ! jq -ne --rawfile q "$quote" '$q | rtrimstr("\n") | split("…") | map(gsub("^\\s+|\\s+$"; "")) | any(. != "")' >/dev/null; then
  echo "引用の文が空" >&2
  exit 2
fi

# -R と fromjson? は、書きかけの行（記録は書き足されている途中のことがある）で止まらないため。-- は、
# ~/.claude/projects の下の - で始まる相対パスを jq がオプションと読まないため
out=$(jq -rR --rawfile q "$quote" '
  ($q | rtrimstr("\n")) as $q
  | ($q | split("…") | map(gsub("^\\s+|\\s+$"; "")) | map(select(. != ""))) as $parts
  | if $parts == [] then "引用の文が空\n" | halt_error(2) else . end
  | def text_of:
      (if type == "string" then [.] elif type == "array" then map(select(.type? == "text") | .text) else [] end)
      | map(gsub("<pasted_content[^>]*>.*?</pasted_content[^>]*>"; ""; "m")
            | if test("<command-args>") then capture("<command-args>(?<a>.*?)</command-args>"; "m").a
              elif test("^\\s*<") then empty else . end)
      | join("\n");
    def in_order($s): reduce $parts[] as $p ($s; if . == null then null
      else split($p) as $x | if ($x | length) < 2 then null else ($x[1:] | join($p)) end end) != null;
    fromjson? | select(type == "object")
    | (if .type == "user" and .origin.kind? == "human" then .message.content
       elif .type == "attachment" and .attachment.type? == "queued_command"
         and (.attachment.commandMode // "prompt") == "prompt"
         and (.attachment.origin.kind? // "human") == "human" then .attachment.prompt
       else empty end | text_of) as $t
    | select($t | in_order(.))
    | "\(input_filename) \(.timestamp) \(.timestamp[0:19] + "Z" | fromdate + 32400 | strftime("%Y-%m-%d %H:%M JST"))\n\($t)\n---"
' -- "$@")
if [[ -z "$out" ]]; then
  exit 1
fi
printf '%s\n' "$out"
