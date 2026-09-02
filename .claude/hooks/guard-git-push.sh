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

# ヒアドキュメント本文はデータであってコマンドではない
# （ドキュメントやコミットメッセージに書かれたコマンド例に反応しないため）
# shellcheck source=lib/heredoc.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/heredoc.sh"
COMMAND=$(mask_heredoc_bodies <<<"$COMMAND")

# 正規表現の部品（block-main-commit.sh と同じ）
OPT='-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?'
PATH_TOKEN='"[^"]*"|'\''[^'\'']*'\''|[^[:space:]]+'

DETECT_RE='(^|[[:space:];&|(])git[[:space:]]+(('"$OPT"')[[:space:]]+)*push([[:space:]]|$|[;&|])'
if [[ ! "$COMMAND" =~ $DETECT_RE ]]; then
  exit 0
fi

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

# open PR のあるブランチへの force push（--force-with-lease）を止める。
# レビュー履歴・コメントの対応行がずれるため、履歴の整理はマージ後か PR を閉じてから行う。
# GitHub リモートがなければ PR の概念がないので何もしない。確認できない場合は
# 通さない（pre-merge-check と同じ方針）
check_open_pr() {
  local branch="$1"
  [[ -n "$branch" ]] || return 0
  git -C "$TARGET_DIR" remote -v 2>/dev/null | grep -q 'github\.com' || return 0
  if ! command -v gh >/dev/null 2>&1; then
    deny "ブランチ ${branch} に open PR があるか確認できません（gh がありません）"
  fi
  local state err_file err
  err_file=$(mktemp)
  if state=$(cd "$TARGET_DIR" && gh pr view "$branch" --json state --jq .state 2>"$err_file"); then
    rm -f "$err_file"
    if [[ "$state" == "OPEN" ]]; then
      deny "open PR のあるブランチ ${branch} への force push は禁止です（レビュー履歴・コメントの対応行がずれる）。履歴の整理はマージ後か PR を閉じてから行ってください"
    fi
  else
    err=$(cat "$err_file")
    rm -f "$err_file"
    if [[ "$err" == *"no pull requests found"* ]]; then
      return 0
    fi
    deny "ブランチ ${branch} に open PR があるか確認できません（gh pr view が失敗: ${err})"
  fi
}

check_push() {
  local PUSH_PART="$1"
  # --- force push ---
  # -f / --force（単独でも -fu のようなクラスタでも）。--force-with-lease / --force-if-includes は対象外
  if echo "$PUSH_PART" | grep -qE '(^|[[:space:]])(--force|-[a-zA-Z]*f[a-zA-Z]*)([[:space:]]|$)'; then
    deny "force push は禁止です（レビュー履歴を保持するため）。feature branch で履歴を書き換える必要がある場合は --force-with-lease を使ってください"
  fi

  # --force-with-lease は feature branch で許可するが、open PR のあるブランチには許可しない
  local LEASE=0
  if echo "$PUSH_PART" | grep -qE '(^|[[:space:]])--force-with-lease(=[^[:space:]]*)?([[:space:]]|$)'; then
    LEASE=1
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
  # プロセス置換の失敗は mapfile に伝わらないため、コマンド置換の終了コードで判定する
  local TOKENS_RAW
  if ! TOKENS_RAW=$(printf '%s' "$PUSH_PART" | xargs -n1 printf '%s\n' 2>/dev/null); then
    deny "git push の引数を解析できません（クォートが不整合）: ${PUSH_PART}"
  fi
  mapfile -t TOKENS <<<"$TOKENS_RAW"
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
      if (( LEASE )); then
        check_open_pr "${dst#refs/heads/}"
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
    if (( LEASE )); then
      check_open_pr "${UPSTREAM:-$CURRENT}"
    fi
  fi
}

# コマンド中の全ての git push を検査する（`git push origin feat && git push origin main`
# のように 2 つ目以降に保護対象が来る形を見逃さない）
REST="$COMMAND"
while [[ "$REST" =~ $DETECT_RE ]]; do
  AFTER="${REST#*"${BASH_REMATCH[0]}"}"
  check_push "${AFTER%%[;&|]*}"
  REST="$AFTER"
done

exit 0
