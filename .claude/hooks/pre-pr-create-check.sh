#!/usr/bin/env bash
# PreToolUse hook: gh pr create の前提条件を検証する
#
# gh-pr-create スキルの手順のうち、コマンド文字列と git の状態だけで決定的に
# 判定できるものをゲートにする（スキルを経由しない gh pr create にも効く）:
#   1. --title がある（--fill は使わない）。70 文字以内
#   2. --body / --body-file がある。本文に ## 概要 / ## 変更点 / ## テスト が揃っている
#      （--body を省くと gh がエディタを開き、Claude Code セッションでは固まる）
#   3. 現在のブランチに上流があり、未プッシュのコミットがない
#      （--head 指定時は別ブランチが対象なので見ない）
#   4. --web を使わない（ブラウザを開かず URL を報告する）
#
# エスケープは設けない（いずれも満たしてから実行し直せばよい）。

set -euo pipefail
# 文字数を UTF-8 の文字単位で数える（日本語タイトルをバイト数で誤って超過させない）
export LC_ALL=C.UTF-8

INPUT=$(cat)
TOOL_NAME=$(jq -r '.tool_name // empty' <<<"$INPUT")
COMMAND=$(jq -r '.tool_input.command // empty' <<<"$INPUT")

[[ "$TOOL_NAME" == "Bash" ]] || exit 0

# gh pr create の検出（行頭だけでなく `cd x && gh pr create` の形も対象）
DETECT_RE='(^|[[:space:];&|(])gh[[:space:]]+pr[[:space:]]+create([[:space:]]|$|[;&|])'
[[ "$COMMAND" =~ $DETECT_RE ]] || exit 0

# create 以降（フラグ解析の対象）
CREATE_PART="${COMMAND#*"${BASH_REMATCH[0]}"}"

REASONS=()

# gh / git の実行コンテキストをコマンドに合わせる（`cd <path> && gh pr create`）
CD_RE='.*(^|&&|;)[[:space:]]*cd[[:space:]]+("[^"]*"|'\''[^'\'']*'\''|[^;&|[:space:]]+)'
if [[ "$COMMAND" =~ $CD_RE ]]; then
  TARGET_DIR="${BASH_REMATCH[2]}"
  TARGET_DIR="${TARGET_DIR%\"}"; TARGET_DIR="${TARGET_DIR#\"}"
  TARGET_DIR="${TARGET_DIR%\'}"; TARGET_DIR="${TARGET_DIR#\'}"
  TARGET_DIR="${TARGET_DIR/#\~/$HOME}"
  if ! cd "$TARGET_DIR" 2>/dev/null; then
    REASONS+=("コマンド中の cd 先に移動できません: ${TARGET_DIR}")
  fi
fi

# フラグの有無（単語境界で判定）
has_flag() {
  local re="(^|[[:space:]])($1)([[:space:]=]|$)"
  [[ "$CREATE_PART" =~ $re ]]
}

# --- 4. --web ---
if has_flag '--web|-w'; then
  REASONS+=("--web は使わないでください（ブラウザを開かず、gh pr create が出力した URL を報告する）")
fi

# --- 1. タイトル ---
TITLE_RE='(^|[[:space:]])(-t|--title)[[:space:]=]+("([^"]*)"|'\''([^'\'']*)'\''|([^[:space:]]+))'
if [[ "$CREATE_PART" =~ $TITLE_RE ]]; then
  TITLE="${BASH_REMATCH[4]}${BASH_REMATCH[5]}${BASH_REMATCH[6]}"
  if (( ${#TITLE} > 70 )); then
    REASONS+=("--title は 70 文字以内にしてください（現在 ${#TITLE} 文字）")
  fi
else
  REASONS+=("--title で PR タイトルを指定してください（--fill でコミットメッセージを流用しない）")
fi

# --- 2. 本文 ---
BODY_TEXT="$CREATE_PART"
BODY_FILE=$(grep -oE '(--body-file|-F)[[:space:]=]+[^[:space:]]+' <<<"$CREATE_PART" | head -n 1 \
  | sed -E 's/^(--body-file|-F)[[:space:]=]+//' || true)
if [[ -n "$BODY_FILE" ]]; then
  BODY_FILE="${BODY_FILE/#\~/$HOME}"
  if [[ -r "$BODY_FILE" ]]; then
    BODY_TEXT=$(cat "$BODY_FILE")
  else
    REASONS+=("--body-file のファイルが読めません: ${BODY_FILE}")
    BODY_TEXT=""
  fi
fi
if ! has_flag '--body|-b|--body-file|-F'; then
  REASONS+=("--body または --body-file で PR 本文を指定してください（## 概要 / ## 変更点 / ## テスト）")
else
  MISSING=()
  for h in '## 概要' '## 変更点' '## テスト'; do
    grep -qF -- "$h" <<<"$BODY_TEXT" || MISSING+=("$h")
  done
  if [[ ${#MISSING[@]} -gt 0 ]]; then
    REASONS+=("PR 本文に見出しがありません: ${MISSING[*]}")
  fi
fi

# --- 3. 未プッシュコミット ---
if ! has_flag '--head|-H'; then
  if ! UPSTREAM=$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null); then
    REASONS+=("現在のブランチに上流ブランチがありません。先に git push してください（gh が対話的に push 先を尋ねて固まるのを防ぐ）")
  else
    AHEAD=$(git rev-list --count '@{u}..HEAD' 2>/dev/null || echo "?")
    if [[ "$AHEAD" != "0" ]]; then
      REASONS+=("未プッシュのコミットが ${AHEAD} 件あります（${UPSTREAM} より先）。先に git push してください")
    fi
  fi
fi

# --- 結果出力 ---
if [[ ${#REASONS[@]} -gt 0 ]]; then
  REASON_TEXT=$(printf '%s\n' "${REASONS[@]}")
  jq -n --arg reason "$REASON_TEXT" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
fi
exit 0
