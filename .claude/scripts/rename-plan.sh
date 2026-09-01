#!/usr/bin/env bash
# rename-plan.sh — docs/plans/ のランダム名の計画書を連番付きの名前にする
#
# Usage: rename-plan.sh --list
#        rename-plan.sh <branch-name> [--file <path>]
#
# Claude Code の plan mode は plansDirectory（docs/plans）にランダム名で計画書を置く。
# ブランチ名が決まったら `NNN_<name>.md` にリネームして追跡しやすくする。
#
# --list: 連番プレフィックス（NNN_）のない .md を新しい順に出す（ブランチ名を決める材料）
# リネーム:
#   - 対象は --file、省略時はランダム名の計画書がちょうど 1 つのときそれ。複数なら exit 1
#   - NNN は既存の最大番号 + 1（なければ 001）
#   - name はブランチ名から先頭の type/ を除き、- と / を _ に変換したもの
#   - git 追跡下なら git mv してコミット。未追跡（gitignore 等）なら mv のみ
#
# 出力:
#   RENAMED: <old> -> <new>
#   COMMITTED: yes|no

set -euo pipefail

usage() {
  echo "Usage: $0 --list | <branch-name> [--file <path>]" >&2
  exit 1
}

ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "ERROR: git リポジトリではありません" >&2
  exit 1
}
cd "$ROOT"
PLANS_DIR="docs/plans"

LIST=0
BRANCH=""
FILE=""
while (( $# > 0 )); do
  case "$1" in
    --list) LIST=1 ;;
    --file)
      FILE="${2:-}"
      [[ -n "$FILE" ]] || usage
      shift
      ;;
    -*) usage ;;
    *)
      [[ -z "$BRANCH" ]] || usage
      BRANCH="$1"
      ;;
  esac
  shift
done

# 連番のない計画書を新しい順に（ルート相対パスで）
list_random() {
  local f
  [[ -d "$PLANS_DIR" ]] || return 0
  # shellcheck disable=SC2012  # 更新順が必要で、パスにスペースは想定しない
  ls -t "$PLANS_DIR"/*.md 2>/dev/null | while read -r f; do
    [[ "$(basename "$f")" =~ ^[0-9]{3}_ ]] || printf '%s\n' "$f"
  done
}

if (( LIST )); then
  list_random
  exit 0
fi

[[ -n "$BRANCH" ]] || usage

if [[ ! -d "$PLANS_DIR" ]]; then
  echo "NOTE: $PLANS_DIR がありません。リネーム対象なし"
  exit 0
fi

if [[ -z "$FILE" ]]; then
  mapfile -t CANDIDATES < <(list_random)
  if (( ${#CANDIDATES[@]} == 0 )); then
    echo "NOTE: リネーム対象の計画書はありません"
    exit 0
  fi
  if (( ${#CANDIDATES[@]} > 1 )); then
    echo "ERROR: ランダム名の計画書が複数あります。--file で対象を指定してください:" >&2
    printf '  %s\n' "${CANDIDATES[@]}" >&2
    exit 1
  fi
  FILE="${CANDIDATES[0]}"
fi
if [[ ! -f "$FILE" ]]; then
  echo "ERROR: ファイルがありません: $FILE" >&2
  exit 1
fi

MAX=0
for f in "$PLANS_DIR"/[0-9][0-9][0-9]_*.md; do
  [[ -e "$f" ]] || continue
  n=$(basename "$f")
  n=$((10#${n:0:3}))
  (( n > MAX )) && MAX=$n
done
NEXT=$(printf '%03d' $((MAX + 1)))

NAME="${BRANCH#*/}"
NAME="${NAME//-/_}"
NAME="${NAME//\//_}"
NEW="$PLANS_DIR/${NEXT}_${NAME}.md"

if [[ -e "$NEW" ]]; then
  echo "ERROR: 既に存在します: $NEW" >&2
  exit 1
fi

if git ls-files --error-unmatch -- "$FILE" >/dev/null 2>&1; then
  git mv -- "$FILE" "$NEW"
  git commit -q --only -m "docs: rename plan $(basename "$FILE" .md) to $(basename "$NEW")" -- "$FILE" "$NEW"
  COMMITTED=yes
else
  mv -- "$FILE" "$NEW"
  COMMITTED=no
fi

echo "RENAMED: $FILE -> $NEW"
echo "COMMITTED: $COMMITTED"
