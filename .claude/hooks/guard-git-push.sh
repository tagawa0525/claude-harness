#!/usr/bin/env bash
# PreToolUse hook: 保護対象への git push をブロックする
#
# CLAUDE.md / git-push スキルのルールをツールレベルで強制する:
#   - main / master への直接 push（PR を経由する）
#   - force push（--force / -f / +refspec）。--force-with-lease は feature branch に限り許可
#   - --all / --mirror（main を含む全ブランチを押し出す）
#
# エスケープ: どうしても必要なときはコマンドに `ALLOW_PROTECTED_PUSH=1` を付ける。
#   例: ALLOW_PROTECTED_PUSH=1 git push origin main
# 環境変数の代入をコマンド文字列に書かせるのは、hook が文字列だけで決定的に判定でき、
# 会話ログに意図が残るため。

set -euo pipefail

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

[[ "$TOOL_NAME" == "Bash" ]] || exit 0

# 正規表現の部品（block-main-commit.sh と同じ）
OPT='-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?'
PATH_TOKEN='"[^"]*"|'\''[^'\'']*'\''|[^[:space:]]+'

DETECT_RE='(^|[[:space:];&|(])git[[:space:]]+(('"$OPT"')[[:space:]]+)*push([[:space:]]|$|[;&|])'
if [[ ! "$COMMAND" =~ $DETECT_RE ]]; then
  exit 0
fi
# 最初に検出した git push の直後から、次のコマンド区切りまでを引数部分とする
PUSH_PART="${COMMAND#*"${BASH_REMATCH[0]}"}"
PUSH_PART="${PUSH_PART%%[;&|]*}"

# エスケープ
if echo "$COMMAND" | grep -qE '(^|[[:space:];&|(])ALLOW_PROTECTED_PUSH=1([[:space:]]|$)'; then
  exit 0
fi

deny() {
  jq -n --arg reason "$1"$'\n\n'"どうしても必要な場合は ALLOW_PROTECTED_PUSH=1 をコマンドに付けて実行してください。" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
  exit 0
}

# 対象ディレクトリ（-C / 最後の cd）
GIT_C_RE='git[[:space:]]+-C[[:space:]]+('"$PATH_TOKEN"')([[:space:]]+'"$OPT"')*[[:space:]]+push'
CD_RE='.*(^|&&|;)[[:space:]]*cd[[:space:]]+("[^"]*"|'\''[^'\'']*'\''|[^;&|[:space:]]+)'
TARGET_DIR="."
if [[ "$COMMAND" =~ $GIT_C_RE ]]; then
  TARGET_DIR="${BASH_REMATCH[1]}"
elif [[ "$COMMAND" =~ $CD_RE ]]; then
  TARGET_DIR="${BASH_REMATCH[2]}"
fi
TARGET_DIR="${TARGET_DIR%\"}"; TARGET_DIR="${TARGET_DIR#\"}"
TARGET_DIR="${TARGET_DIR%\'}"; TARGET_DIR="${TARGET_DIR#\'}"
TARGET_DIR="${TARGET_DIR/#\~/$HOME}"

# --- force push ---
# -f / --force（単独でも -fu のようなクラスタでも）。--force-with-lease / --force-if-includes は対象外
if echo "$PUSH_PART" | grep -qE '(^|[[:space:]])(--force|-[a-zA-Z]*f[a-zA-Z]*)([[:space:]]|$)'; then
  deny "force push は禁止です（レビュー履歴を保持するため）。feature branch で履歴を書き換える必要がある場合は --force-with-lease を使ってください"
fi

# --- --all / --mirror ---
if echo "$PUSH_PART" | grep -qE '(^|[[:space:]])(--all|--mirror)([[:space:]]|$)'; then
  deny "--all / --mirror は main を含む全ブランチを push するため禁止です"
fi

# --- main / master への push ---
# 引数の refspec を集める（フラグと remote 名を除く）。`+refspec` は force 扱い
# クォート付き引数（例: "/path with space"）を壊さないよう xargs でシェル風に分割する
# （xargs はクォートとバックスラッシュを解釈するだけで、コマンド置換等は実行しない）。
# 分割に失敗（クォート不整合）した場合は解析不能として deny する
if ! mapfile -t TOKENS < <(printf '%s' "$PUSH_PART" | xargs -n1 printf '%s\n' 2>/dev/null); then
  deny "git push の引数を解析できません（クォートが不整合）: ${PUSH_PART}"
fi
REFSPECS=()
SKIP_NEXT=0
POSITIONAL=0
for tok in "${TOKENS[@]}"; do
  if (( SKIP_NEXT )); then SKIP_NEXT=0; continue; fi
  case "$tok" in
    -o|--push-option|--receive-pack|--exec|--repo) SKIP_NEXT=1 ;;
    --repo=*) ;;
    -*) ;;
    *)
      POSITIONAL=$((POSITIONAL + 1))
      # 1つ目の位置引数は remote。2つ目以降が refspec
      if (( POSITIONAL >= 2 )); then REFSPECS+=("$tok"); fi
      ;;
  esac
done

is_protected_ref() {
  local dst="$1"
  dst="${dst#refs/heads/}"
  [[ "$dst" == "main" || "$dst" == "master" ]]
}

if [[ ${#REFSPECS[@]} -gt 0 ]]; then
  for rs in "${REFSPECS[@]}"; do
    if [[ "$rs" == +* ]]; then
      deny "+refspec による force push は禁止です: ${rs}"
    fi
    dst="${rs#*:}"          # src:dst → dst、単独なら全体
    [[ "$rs" == *:* ]] || dst="$rs"
    if [[ "$dst" == "HEAD" || -z "$dst" ]]; then
      # `git push origin HEAD` / `git push origin :branch`（削除）は現在ブランチ／削除対象で判定
      if [[ "$dst" == "HEAD" ]]; then
        dst=$(git -C "$TARGET_DIR" branch --show-current 2>/dev/null || echo "")
      else
        continue
      fi
    fi
    if is_protected_ref "$dst"; then
      deny "main / master への直接 push は禁止です（refspec: ${rs}）。feature branch から PR を作成してください: /gh-pr-create"
    fi
  done
else
  # refspec なし → 現在のブランチ（upstream 名が異なる場合は upstream で判定）
  CURRENT=$(git -C "$TARGET_DIR" branch --show-current 2>/dev/null || echo "")
  UPSTREAM=$(git -C "$TARGET_DIR" rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || echo "")
  UPSTREAM="${UPSTREAM#*/}"
  if is_protected_ref "$CURRENT" || is_protected_ref "$UPSTREAM"; then
    deny "main / master への直接 push は禁止です（現在のブランチ: ${CURRENT}）。feature branch から PR を作成してください: /gh-pr-create"
  fi
fi

exit 0
