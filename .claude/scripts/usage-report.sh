#!/bin/sh
# usage-report.sh — Claude Code の会話の記録から、区間の費用（ドル）を親・担当・レビュー・振り返りに分けて出す。中身は
# claude-hooks script usage-report（Rust にして Windows でも動かす。ADR-0088）
#
# Usage: usage-report.sh [--since <日時>] [--until <日時>] [--repo <リポジトリ>]... [TRANSCRIPT]
#   TRANSCRIPT  セッションの記録（~/.claude/projects/<プロジェクト>/<セッション>.jsonl）。
#               省略時は CLAUDE_CODE_SESSION_ID の記録を ~/.claude/projects/*/ から探す
#   --since, --until
#               応答の timestamp が区間 [since, until) に入るものだけを数える。日時は ISO 8601 で、時差
#               （Z か +09:00 の形）を必ず付ける（シェルの TZ で意味が変わらないため）。両方を省くと JST の今日、
#               片方だけなら、もう片方の端は開いたまま
#   --repo      マージの数を数えるリポジトリ（複数可）。origin/HEAD があればそれ、無ければ主の worktree のブランチの
#               first-parent のマージコミットのうち、コミットの日時が区間に入るものを数える。origin/HEAD は
#               fetch しないと古いので、呼ぶ前に fetch する
#
# サブエージェントの記録の読み方と区分の決め方は、claude-hooks の src/script/usage_report.rs の冒頭
#
# 出力:
#   TRANSCRIPT: <path>
#   SINCE: <JST の日時>  UNTIL: <JST の日時>   区間の端（指定した端だけ）
#   TOTAL: $<合計>
#   COST: <区分> <本数> 本 $<費用> <割合>% 書き直し <回数> 回 $<その書き込みの費用>
#   TOKENS: <区分> input=<数> write_5m=<数> write_1h=<数> read=<数> output=<数>
#   REWRITES: <回数> 回 $<費用>   文脈の書き直し（1 回の応答で書き込みが 25 万トークンを超えたもの）の合計
#   MERGES: <数> <リポジトリ> <ref>   --repo ごと
#   PER_MERGE: $<費用>（マージ <数> 本） | -（--repo なし） | -（マージ 0 本）
#   TOP: $<費用> <区分> <応答の数> 回 文脈の最大 <トークン> <始め>-<終わり> <依頼文の頭>   費用の大きい
#                                      サブエージェント 10 本
#
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script usage-report "$@"
