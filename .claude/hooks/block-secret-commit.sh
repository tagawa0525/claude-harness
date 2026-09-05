#!/usr/bin/env bash
# PreToolUse hook: 機密情報を含む変更の git commit をブロック
#
# コミット対象（ステージ済み。-a / --all ならワーキングツリーの追跡ファイルの変更も）を検査し、
# 次のいずれかが見つかれば deny する:
#   1. ファイル名: .env / .env.<name>（.example / .sample / .template / .dist の雛形は除く）、
#      秘密鍵（*.pem *.key *.p12 *.pfx *.jks *.keystore、id_rsa / id_dsa / id_ecdsa / id_ed25519）、
#      .netrc / .pypirc、*.tfstate
#   2. 追加行の内容: 秘密鍵本文、形式の決まったトークン（AWS / GitHub / Slack / OpenAI・Anthropic /
#      Google / Stripe）、password / secret / token 等への文字列代入（16 文字以上でプレースホルダでない）
#
# 検査するのは追加行だけ（秘密を消す変更は通す）。理由には path:line と種別だけを出し、
# 値そのものは出さない（会話ログに秘密を残さない）。
#
# 誤検出の除外は 2 段階:
#   - その行に `gitleaks:allow` を書く（gitleaks と同じ慣習。将来 gitleaks に替えても互換）
#   - コマンドに `ALLOW_SECRET_COMMIT=1` を付ける（guard-git-push.sh の ALLOW_PROTECTED_PUSH と同じ方式）
#
# 判定は git と awk だけで行う（外部ツール不要）。網羅性より誤検出の少なさを優先し、
# 形式が決まっているトークンと明らかな秘密鍵に絞る。

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
PATH_TOKEN='"[^"]*"|'\''[^'\'']*'\''|[^[:space:]]+'

# エスケープ（=~ は BASH_REMATCH を上書きするので、検出より前に grep で判定する）
if grep -qE '(^|[[:space:];&|(])ALLOW_SECRET_COMMIT=1([[:space:]]|$)' <<<"$COMMAND"; then
  exit 0
fi

DETECT_RE='(^|[[:space:];&|(])git[[:space:]]+(('"$OPT"')[[:space:]]+)*commit([[:space:]]|$|[;&|])'
[[ "$COMMAND" =~ $DETECT_RE ]] || exit 0

# commit 以降（-a の検出対象。次のコマンド区切りまで）
COMMIT_PART="${COMMAND#*"${BASH_REMATCH[0]}"}"
COMMIT_PART="${COMMIT_PART%%[;&|]*}"

# 対象ディレクトリ（-C / 最後の cd）
GIT_C_RE='git[[:space:]]+-C[[:space:]]+('"$PATH_TOKEN"')([[:space:]]+'"$OPT"')*[[:space:]]+commit'
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

# -a / --all ならワーキングツリーの追跡ファイルの変更も対象（warn-large-commit.sh と同じ判定）
ALL_RE='(^|[[:space:]])(--all|-[a-zA-Z]*a[a-zA-Z]*)([[:space:]]|$)'
DIFF_ARGS=(--cached)
if [[ "$COMMIT_PART" =~ $ALL_RE ]]; then
  DIFF_ARGS=(HEAD)
fi

FILES=$(git -C "$TARGET_DIR" diff "${DIFF_ARGS[@]}" --name-only --diff-filter=ACMR 2>/dev/null || true)
[[ -n "$FILES" ]] || exit 0

FINDINGS=()

# --- 1. ファイル名 ---
while IFS= read -r path; do
  base=$(basename "$path")
  case "$base" in
    *.example|*.sample|*.template|*.dist) ;;
    .env|.env.*) FINDINGS+=("${path} (環境変数ファイル)") ;;
    *.pem|*.key|*.p12|*.pfx|*.jks|*.keystore) FINDINGS+=("${path} (秘密鍵・証明書ストア)") ;;
    id_rsa|id_dsa|id_ecdsa|id_ed25519) FINDINGS+=("${path} (SSH 秘密鍵)") ;;
    .netrc|.pypirc) FINDINGS+=("${path} (認証情報ファイル)") ;;
    *.tfstate|*.tfstate.backup) FINDINGS+=("${path} (Terraform state)") ;;
  esac
done <<<"$FILES"

# --- 2. 追加行の内容 ---
# 追加行を「path<TAB>line<TAB>内容」に展開する（-U0 なので文脈行は出ない）。
# `gitleaks:allow` を含む行は検査対象から外す
ADDED=$(git -C "$TARGET_DIR" diff "${DIFF_ARGS[@]}" -U0 --no-color --diff-filter=ACMR 2>/dev/null \
  | awk '
      /^\+\+\+ / { file = $0; sub(/^\+\+\+ b\//, "", file); next }
      /^@@ /     { match($0, /\+[0-9]+/); line = substr($0, RSTART + 1, RLENGTH - 1) + 0; next }
      /^\+/      { if ($0 !~ /gitleaks:allow/) printf "%s\t%d\t%s\n", file, line, substr($0, 2); line++; next }
    ' || true)

# check_rule <種別> <ERE> [generic]
#   generic を付けると、小文字化した行で照合し、マッチした値がプレースホルダなら除外する
check_rule() {
  local label="$1" re="$2" mode="${3:-}"
  local hits
  [[ -n "$ADDED" ]] || return 0
  hits=$(awk -F'\t' -v re="$re" -v mode="$mode" '
    {
      c = $0; sub(/^[^\t]*\t[^\t]*\t/, "", c)
      if (mode == "generic") {
        c = tolower(c)
        if (match(c, re)) {
          v = substr(c, RSTART, RLENGTH)
          if (v ~ /example|changeme|placeholder|dummy|xxx|redacted|your[_-]|todo|<|>|\$\{|\$\(|\*\*\*/) next
          print $1 ":" $2
        }
      } else if (c ~ re) {
        print $1 ":" $2
      }
    }' <<<"$ADDED")
  [[ -n "$hits" ]] || return 0
  while IFS= read -r h; do FINDINGS+=("${h} (${label})"); done <<<"$hits"
}

check_rule "秘密鍵本文"            '-----BEGIN[ A-Z]*PRIVATE KEY( BLOCK)?-----'
check_rule "AWS アクセスキー"      '(^|[^A-Za-z0-9])(AKIA|ASIA)[0-9A-Z]{16}([^A-Za-z0-9]|$)'
check_rule "GitHub トークン"       '(^|[^A-Za-z0-9_])(gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})'
check_rule "Slack トークン"        'xox[baprs]-[0-9A-Za-z-]{10,}'
check_rule "OpenAI/Anthropic キー" '(^|[^A-Za-z0-9_-])sk-(ant-)?[A-Za-z0-9_-]{20,}'
check_rule "Google API キー"       'AIza[0-9A-Za-z_-]{35}'
check_rule "Stripe キー"           '(^|[^A-Za-z0-9_])(sk|rk)_(live|test)_[0-9a-zA-Z]{24,}'
check_rule "秘密らしき値の代入" \
  '(password|passwd|pwd|secret|api[_-]?key|access[_-]?key|auth[_-]?token|token)[a-z0-9_]*[[:space:]]*[:=][[:space:]]*["'\''][^"'\'']{16,}["'\'']' generic

(( ${#FINDINGS[@]} > 0 )) || exit 0

REASON="機密情報らしき内容がコミット対象に含まれています:"$'\n'
REASON+=$(printf -- '- %s\n' "${FINDINGS[@]}")
REASON+=$'\n\n'"対処: 該当ファイルを git restore --staged <file> で外し、必要なら .gitignore に追加してください。"
REASON+=$'\n'"既に push 済みの値はローテーションしてください。"
REASON+=$'\n'"誤検出なら、その行に gitleaks:allow を書くか、コマンドに ALLOW_SECRET_COMMIT=1 を付けて実行してください。"

jq -n --arg reason "$REASON" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: $reason
  }
}'
exit 0
