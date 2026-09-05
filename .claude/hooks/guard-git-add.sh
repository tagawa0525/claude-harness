#!/usr/bin/env bash
# PreToolUse hook: 対象を絞らない git add をブロック
#
# `git add -A` / `--all` / `.` / `:/` / `*` は、未追跡ファイル（.env や鍵など）を
# まとめてステージするため禁止する。block-secret-commit.sh（内容の検査）と対で効く。
# 1 コミット 1 論理変更のためにも、`git add <file>` / `git add -p` で対象を選ぶ。
#
# 通すもの:
#   - パスを指定した add（`git add src/a.txt`、`git add -A src/` のように範囲を限定した -A も可）
#   - `-u`（追跡済みファイルだけが対象で、未追跡は入らない）
#   - `-p` / `-i`（対話的に選ぶ）
#
# エスケープ: コマンドに `ALLOW_GIT_ADD_ALL=1` を付ける（guard-git-push.sh と同じ方式）

set -euo pipefail

INPUT=$(cat)
TOOL_NAME=$(jq -r '.tool_name // empty' <<<"$INPUT")
COMMAND=$(jq -r '.tool_input.command // empty' <<<"$INPUT")

[[ "$TOOL_NAME" == "Bash" ]] || exit 0

# ヒアドキュメント本文はデータであってコマンドではない
# shellcheck source=lib/heredoc.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/heredoc.sh"
COMMAND=$(mask_heredoc_bodies <<<"$COMMAND")

# 正規表現の部品（block-main-commit.sh と同じ）
OPT='-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?'

DETECT_RE='(^|[[:space:];&|(])git[[:space:]]+(('"$OPT"')[[:space:]]+)*add([[:space:]]|$|[;&|])'
grep -qE "$DETECT_RE" <<<"$COMMAND" || exit 0

# エスケープ
if grep -qE '(^|[[:space:];&|(])ALLOW_GIT_ADD_ALL=1([[:space:]]|$)' <<<"$COMMAND"; then
  exit 0
fi

deny() {
  jq -n --arg reason "$1" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
  exit 0
}

check_add() {
  local ADD_PART="$1"
  local TOKENS_RAW tok all=0 after_dashdash=0
  local -a PATHSPECS=()
  # クォート付き引数を壊さないよう xargs でシェル風に分割する（guard-git-push.sh と同じ）
  if ! TOKENS_RAW=$(printf '%s' "$ADD_PART" | xargs -n1 printf '%s\n' 2>/dev/null); then
    deny "git add の引数を解析できません（クォートが不整合）: ${ADD_PART}"
  fi
  [[ -n "$TOKENS_RAW" ]] || return 0
  while IFS= read -r tok; do
    if (( after_dashdash )); then PATHSPECS+=("$tok"); continue; fi
    case "$tok" in
      --) after_dashdash=1 ;;
      --all|--no-ignore-removal) all=1 ;;
      --*) ;;
      -*A*) all=1 ;;   # -A や -An のようなクラスタ
      -*) ;;
      *) PATHSPECS+=("$tok") ;;
    esac
  done <<<"$TOKENS_RAW"

  local why=""
  if (( all )) && (( ${#PATHSPECS[@]} == 0 )); then
    why="git add -A / --all はパスを指定しない限り禁止です"
  else
    local p
    for p in "${PATHSPECS[@]}"; do
      case "$p" in
        .|./|:/|'*') why="git add ${p} は禁止です" ; break ;;
      esac
    done
  fi
  [[ -n "$why" ]] || return 0

  deny "${why}（未追跡ファイルをまとめてステージし、機密情報の混入と無関係な変更の混在を招くため）。
git status で対象を確認し、git add <file> または git add -p で論理単位ごとにステージしてください。
どうしても必要な場合は ALLOW_GIT_ADD_ALL=1 をコマンドに付けて実行してください。"
}

# コマンド中の全ての git add を検査する（`git add a && git add -A` の 2 つ目も見逃さない）
REST="$COMMAND"
while [[ "$REST" =~ $DETECT_RE ]]; do
  AFTER="${REST#*"${BASH_REMATCH[0]}"}"
  check_add "${AFTER%%[;&|]*}"
  REST="$AFTER"
done

exit 0
