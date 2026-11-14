#!/bin/sh
# session-events.sh — Claude Code の会話の記録から、振り返りの手がかり（拒否、失敗、ユーザーの発言）を集める。中身は
# claude-hooks script session-events（Rust にして Windows でも jq なしで動かす）
#
# Usage: session-events.sh [--since <日時>] [--until <日時>] [TRANSCRIPT]
#   TRANSCRIPT  セッションの記録（~/.claude/projects/<プロジェクト>/<セッション>.jsonl）。
#               省略時は CLAUDE_CODE_SESSION_ID の記録を ~/.claude/projects/*/ から探す
#   --since, --until
#               各行の timestamp（UTC）が区間 [since, until) に入る行だけを数える（本体とサブエージェントの両方）。
#               片方だけなら、もう片方の端は開いたまま。隣り合う区間で同じ行を 2 度数えないよう、until は含まない。
#               日時は ISO 8601 で、時差（Z か +09:00 の形）を必ず付ける（例 2026-11-06T00:00+09:00、
#               2026-11-05T15:00:00.000Z）。時差の無い日時はシェルの TZ で意味が変わるので失敗にする。
#               区間を指定したときは、timestamp の無い行（api-request-blob など）を数えず、区間に行の無い
#               サブエージェントの記録は読まない（TRANSCRIPTS にも数えない）。区間に入る行が本体にも
#               サブエージェントにも無ければ、区間の書き誤りとみなして exit 1
#
# 記録の隣の <セッション>/subagents/*.jsonl（サブエージェントの記録）も読む。
#
# 出力:
#   TRANSCRIPT: <path>
#   SINCE: <UTC の日時>  UNTIL: <UTC の日時>   指定した区間の端を UTC に直したもの（指定した端だけ）
#   TRANSCRIPTS: <読んだ記録の数（本体 + サブエージェント）>
#   <KIND>: <回数> <本文の要約>   失敗したツールの結果を、種類と要約ごとに回数の多い順に（同じ回数は要約のバイトの順）
#     HOOK        hook の拒否（`<event>:<tool> hook error: ` を除いた 1 行目）。同じ拒否をまとめるため、
#                 コミットの番号（数字と a-f の両方を含む 7〜40 文字）を `<hash>`、英数字と `-` `#` に続かない数を `N` に置き換える（`ADR-` の後ろの数のように、`-` に続く数は残す）
#     PERMISSION  権限の拒否（1 行目が自動モードの分類器などの拒否）
#     REJECTED    ユーザーが止めたツール（止めたときに書いた言葉があれば `the user said: <言葉>`）
#     TOOL        そのほか（`Exit code N` は、エラーらしい行の最後、無ければ最後の行とつなぐ。
#                 `<tool_use_error>` のタグは除く）
#   USER: <発言>  本体の記録のユーザーの発言を順に（改行は空白に）。ターンの途中で送った発言
#                 （attachment の queued_command のうち人から届いたもの）を含み、スラッシュコマンドは `/<名前> <引数>` にする。
#                 通知（`<` で始まる）、スキルの本文（isMeta）、要約（isCompactSummary）、
#                 サブエージェントへの依頼文は除く
# 要約は 160 文字、発言は 300 文字で切る。JSON として読めない行（走っている最中のセッションの
# 書きかけの最後の行など）は飛ばす。
#
# 記録の形は Claude Code の公開の仕様ではない。type を持つ行が 1 つも無いか、
# ツールを呼んだ行があるのに tool_result が 1 つも無ければ、形が変わったとみなして exit 1（0 件と出さない）。
# user の行の message が object でない、発言の本文が文字列でも配列でもない、なども同じく exit 1。
# 区間を指定したときは、timestamp が YYYY-MM-DDTHH:MM:SS.mmmZ の形でない行があっても exit 1
# （区間の端と文字列で比べるので、形が違うと黙って区間の外に落ちる）。
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script session-events "$@"
