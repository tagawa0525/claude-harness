#!/usr/bin/env bash
set -euo pipefail

# PreToolUse hook: gh pr merge 実行前にマージの前提条件を検証する。
#
# CLAUDE.md の必須ゲートをツールレベルで強制する:
#   1. マージ方式は --merge のみ（--squash / --rebase は禁止）
#   2. --delete-branch でマージ後のブランチを削除する
#   3. マージコミット本文に ## Why / ## What / ## Impact が揃っている
#   4. CI チェックが未完了・失敗していない
#   5. reviewDecision が CHANGES_REQUESTED / REVIEW_REQUIRED でない
#   6. 未解決のレビュースレッドがない
#
# 1〜3 はコマンド文字列だけで判定する。4〜6 は gh で GitHub に問い合わせる。
# 6 は問い合わせに失敗したら deny する（確認できない状態でマージさせない）。

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

# gh pr merge の検出。行頭だけでなく `cd x && gh pr merge` のようにコマンド区切りの
# 後ろにある場合も対象にする（行頭限定だと cd 付きで迂回できてしまう）
DETECT_RE='(^|[[:space:];&|(])gh[[:space:]]+pr[[:space:]]+merge([[:space:]]|$|[;&|])'
if [[ ! "$COMMAND" =~ $DETECT_RE ]]; then
  exit 0
fi

# merge 以降の部分（フラグ解析の対象）
MERGE_PART="${COMMAND#*"${BASH_REMATCH[0]}"}"

# PR番号: 本文（--body/-b/--subject/-t 以降）より前のトークンから、値を取るフラグ
# （-R/--repo/-F/--body-file とその値）を飛ばして最初の数値を採る。
# `gh pr merge -R owner/repo 12 …` のように番号が merge の直後に来ない形にも対応する
PR_REF=""
SKIP_NEXT=0
for tok in $MERGE_PART; do
  if (( SKIP_NEXT )); then SKIP_NEXT=0; continue; fi
  case "$tok" in
    --body|-b|--subject|-t|--body=*|--subject=*) break ;;
    -R|--repo|-F|--body-file) SKIP_NEXT=1 ;;
    --repo=*|--body-file=*) ;;
    -*) ;;
    *)
      if [[ "$tok" =~ ^[0-9]+$ ]]; then PR_REF="$tok"; break; fi
      ;;
  esac
done

REASONS=()

# gh の実行コンテキストをコマンドに合わせる。
# - `cd <path> && gh pr merge` → 最後の cd 先で gh を実行（block-main-commit と同方式）
# - `-R/--repo owner/name`     → そのリポジトリを対象にする
# hook の cwd のまま実行すると、別ディレクトリを対象にしたコマンドで誤った PR を見る
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

REPO_ARGS=()
REPO_RE='(^|[[:space:]])(-R|--repo)[[:space:]=]+([^[:space:]]+)'
if [[ "$MERGE_PART" =~ $REPO_RE ]]; then
  REPO_ARGS=(-R "${BASH_REMATCH[3]}")
fi

# フラグの有無（単語境界で判定。-m のような短縮形も許容）
has_flag() {
  local re="(^|[[:space:]])($1)([[:space:]=]|$)"
  [[ "$MERGE_PART" =~ $re ]]
}

# --- 1. マージ方式 ---
if has_flag '--squash|-s|--rebase|-r'; then
  REASONS+=("--squash / --rebase は禁止です。--merge（マージコミット方式）を使ってください")
elif ! has_flag '--merge|-m'; then
  REASONS+=("--merge を明示してください（マージ方式の既定値に依存しない）")
fi

# --- 2. ブランチ削除 ---
if ! has_flag '--delete-branch|-d'; then
  REASONS+=("--delete-branch を付けてください（マージ後のブランチは速やかに削除する）")
fi

# --- 3. 本文形式 ---
BODY_TEXT="$MERGE_PART"
BODY_FILE=$(echo "$MERGE_PART" | grep -oE '(--body-file|-F)[[:space:]=]+[^[:space:]]+' | head -n 1 | sed -E 's/^(--body-file|-F)[[:space:]=]+//' || true)
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
  REASONS+=("--body でマージコミット本文を指定してください（## Why / ## What / ## Impact）")
else
  MISSING=()
  for h in '## Why' '## What' '## Impact'; do
    grep -qF -- "$h" <<<"$BODY_TEXT" || MISSING+=("$h")
  done
  if [[ ${#MISSING[@]} -gt 0 ]]; then
    REASONS+=("マージコミット本文に見出しがありません: ${MISSING[*]}")
  fi
fi

# 対象リポジトリ（base）。check-run と review thread はどちらも base 側に紐づく
# gh repo view は -R を受け付けない（位置引数）ため、-R 指定時はその文字列から取る
if [[ ${#REPO_ARGS[@]} -gt 0 ]]; then
  OWNER="${REPO_ARGS[1]%%/*}"
  NAME="${REPO_ARGS[1]#*/}"
  NAME="${NAME%.git}"
else
  OWNER=$(gh repo view --json owner --jq '.owner.login' 2>/dev/null || true)
  NAME=$(gh repo view --json name --jq '.name' 2>/dev/null || true)
fi

# --- 4. CI チェック ---
# `gh pr checks --json` は「チェックなし」を exit 1 + メッセージで返し API エラーと
# 区別できない（さらに以前使っていた status フィールドは存在せず常に失敗していた）。
# head コミットの check-runs / commit status を REST で直接読み、取得失敗は deny する
CHECK_ARGS=("${REPO_ARGS[@]}")
if [[ -n "${PR_REF:-}" ]]; then
  CHECK_ARGS+=("$PR_REF")
fi

PR_META=$(gh pr view "${CHECK_ARGS[@]}" --json number,headRefOid,reviewDecision 2>/dev/null || true)
if [[ -z "$PR_META" || -z "$OWNER" || -z "$NAME" ]]; then
  REASONS+=("PR 情報を取得できません（gh pr view が失敗。PR番号・認証・ネットワークを確認）")
else
  # fork からの PR でも check-run は base リポジトリの head SHA に紐づく
  HEAD_SHA=$(jq -r '.headRefOid' <<<"$PR_META")
  CHECK_RUNS=$(gh api --paginate "repos/${OWNER}/${NAME}/commits/${HEAD_SHA}/check-runs?per_page=100" \
    --jq '.check_runs[] | {name, status, conclusion}' 2>/dev/null | jq -s '.' || true)
  STATUSES=$(gh api "repos/${OWNER}/${NAME}/commits/${HEAD_SHA}/status" \
    --jq '[.statuses[] | {name: .context, status: (if .state == "pending" then "in_progress" else "completed" end), conclusion: .state}]' 2>/dev/null || true)
  if [[ -z "$CHECK_RUNS" || -z "$STATUSES" ]]; then
    REASONS+=("CI チェック状態を取得できません（check-runs / status API が失敗）")
  else
    ALL_CHECKS=$(jq -s 'add' <<<"$CHECK_RUNS $STATUSES")
    PENDING=$(jq '[.[] | select(.status != "completed")] | length' <<<"$ALL_CHECKS")
    if [[ "$PENDING" -gt 0 ]]; then
      PENDING_NAMES=$(jq -r '[.[] | select(.status != "completed") | .name] | unique | join(", ")' <<<"$ALL_CHECKS")
      REASONS+=("実行中/待機中のチェックがあります (${PENDING}件): ${PENDING_NAMES}")
    fi
    FAILED=$(jq '[.[] | select(.status == "completed" and (.conclusion | IN("success","neutral","skipped") | not))] | length' <<<"$ALL_CHECKS")
    if [[ "$FAILED" -gt 0 ]]; then
      FAILED_NAMES=$(jq -r '[.[] | select(.status == "completed" and (.conclusion | IN("success","neutral","skipped") | not)) | "\(.name) (\(.conclusion))"] | unique | join(", ")' <<<"$ALL_CHECKS")
      REASONS+=("失敗したチェックがあります (${FAILED}件): ${FAILED_NAMES}")
    fi
  fi
fi

# --- 5. レビュー判定 ---
# PR_META（取得失敗は 4 で deny 済み）から読む。空文字は「レビュー要件なし」で正常
REVIEW_DECISION=$(jq -r '.reviewDecision // ""' <<<"${PR_META:-null}")

if [[ "$REVIEW_DECISION" == "CHANGES_REQUESTED" ]]; then
  REASONS+=("レビューで変更が要求されています (CHANGES_REQUESTED)")
fi

if [[ "$REVIEW_DECISION" == "REVIEW_REQUIRED" ]]; then
  REASONS+=("必須レビューが未完了です (REVIEW_REQUIRED)")
fi

# --- 6. 未解決レビュースレッド ---
# Copilot のレビューは COMMENTED で提出され reviewDecision を変えないため、
# 指摘への対応漏れはスレッドの resolve 状態で判定する（対応後は
# gh-pr-review の resolve-thread.sh で resolve する運用）
PR_NUMBER="${PR_REF:-}"
if [[ -z "$PR_NUMBER" ]]; then
  PR_NUMBER=$(gh pr view "${REPO_ARGS[@]}" --json number --jq '.number' 2>/dev/null || true)
fi
if [[ -z "$PR_NUMBER" ]]; then
  REASONS+=("対象 PR を特定できません（PR番号を指定するか、PR のあるブランチで実行してください）")
else
  # --paginate は $endCursor 変数と pageInfo を使って全ページを辿る。--jq はページごとに
  # 適用されるので、ページ単位の配列を jq -s add で 1 つにまとめる
  # gh はエラー時に生の応答（errors 配列入り）を標準出力に出すため、終了コードと
  # 「結果が JSON 配列であること」の両方を確認してから件数を数える
  UNRESOLVED=""
  if RAW_THREADS=$(gh api graphql --paginate -F owner="$OWNER" -F name="$NAME" -F number="$PR_NUMBER" -f query='
    query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $number) {
          reviewThreads(first: 100, after: $endCursor) {
            pageInfo { hasNextPage endCursor }
            nodes { isResolved path }
          }
        }
      }
    }' --jq '[.data.repository.pullRequest.reviewThreads.nodes[] | select(.isResolved | not) | .path]' 2>/dev/null); then
    UNRESOLVED=$(jq -s 'add // []' <<<"$RAW_THREADS" 2>/dev/null || true)
    jq -e 'type == "array"' <<<"$UNRESOLVED" >/dev/null 2>&1 || UNRESOLVED=""
  fi
  if [[ -z "$UNRESOLVED" ]]; then
    REASONS+=("未解決レビュースレッドを確認できませんでした（gh api graphql が失敗）")
  else
    UNRESOLVED_COUNT=$(jq 'length' <<<"$UNRESOLVED")
    if [[ "$UNRESOLVED_COUNT" -gt 0 ]]; then
      UNRESOLVED_PATHS=$(jq -r 'unique | join(", ")' <<<"$UNRESOLVED")
      REASONS+=("未解決のレビュースレッドがあります (${UNRESOLVED_COUNT}件): ${UNRESOLVED_PATHS}。対応して返信し、resolve-thread.sh で resolve してください")
    fi
  fi
fi

# --- 結果出力 ---
if [[ ${#REASONS[@]} -gt 0 ]]; then
  REASON_TEXT=$(printf '%s\n' "${REASONS[@]}")
  REASON_TEXT="${REASON_TEXT}"$'\n\n'"/gh-actions-check で状況を診断してください。"

  jq -n --arg reason "$REASON_TEXT" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
  exit 0
fi

# 全ゲート通過 → マージ許可
exit 0
