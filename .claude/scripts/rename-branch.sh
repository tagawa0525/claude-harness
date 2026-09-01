#!/usr/bin/env bash
# rename-branch.sh — 現在の feature ブランチをリネームする
#
# Usage: rename-branch.sh <new-name> [--remote]
#
# - main / master ではリネームしない（exit 1）
# - リモートに同名ブランチがあれば、--remote なしでは何もせず止まる（exit 2）。
#   リモートを書き換える操作なので、ユーザーの承認を得てから --remote を付けて再実行する
# - --remote 指定時: 新名を push（上流を設定）してから旧名をリモートから削除する。
#   ただし旧名を head とする open PR があれば、削除で PR が閉じてしまうため中止する（exit 1）
#
# 出力:
#   RENAMED: <old> -> <new>
#   REMOTE: updated|none

set -euo pipefail

usage() {
  echo "Usage: $0 <new-name> [--remote]" >&2
  exit 1
}

NEW=""
REMOTE_OK=0
while (( $# > 0 )); do
  case "$1" in
    --remote) REMOTE_OK=1 ;;
    -*) usage ;;
    *)
      [[ -z "$NEW" ]] || usage
      NEW="$1"
      ;;
  esac
  shift
done
[[ -n "$NEW" ]] || usage

OLD=$(git branch --show-current)
if [[ -z "$OLD" ]]; then
  echo "ERROR: detached HEAD ではリネームできません" >&2
  exit 1
fi
if [[ "$OLD" == "main" || "$OLD" == "master" ]]; then
  echo "ERROR: $OLD はリネームしません。feature ブランチで実行してください" >&2
  exit 1
fi
if [[ "$OLD" == "$NEW" ]]; then
  echo "ERROR: 現在のブランチ名と同じです: $NEW" >&2
  exit 1
fi

REMOTE=$(git config "branch.$OLD.remote" 2>/dev/null || echo origin)
HAS_REMOTE=0
if git remote get-url "$REMOTE" >/dev/null 2>&1 \
  && [[ -n "$(git ls-remote --heads "$REMOTE" "$OLD" 2>/dev/null)" ]]; then
  HAS_REMOTE=1
fi

if (( HAS_REMOTE )) && ! (( REMOTE_OK )); then
  echo "STOP: リモート $REMOTE に $OLD が存在します。リネームするとリモートも更新されます。"
  echo "      続行するにはユーザーの承認を得てから --remote を付けて再実行してください"
  exit 2
fi

if (( HAS_REMOTE )) && git remote -v | grep -q 'github\.com'; then
  if ! command -v gh >/dev/null 2>&1; then
    echo "ERROR: gh がないため $OLD に open PR があるか確認できません" >&2
    exit 1
  fi
  ERR_FILE=$(mktemp)
  trap 'rm -f "$ERR_FILE"' EXIT
  if STATE=$(gh pr view "$OLD" --json state --jq .state 2>"$ERR_FILE"); then
    if [[ "$STATE" == "OPEN" ]]; then
      echo "ERROR: $OLD を head とする open PR があります。リモートのブランチを消すと PR が閉じるため、リネームしません" >&2
      exit 1
    fi
  elif ! grep -q 'no pull requests found' "$ERR_FILE"; then
    echo "ERROR: $OLD の open PR の有無を確認できません: $(cat "$ERR_FILE")" >&2
    exit 1
  fi
fi

git branch -m "$OLD" "$NEW"

if (( HAS_REMOTE )); then
  git push -q -u "$REMOTE" "$NEW"
  git push -q "$REMOTE" --delete "$OLD"
  echo "RENAMED: $OLD -> $NEW"
  echo "REMOTE: updated"
else
  echo "RENAMED: $OLD -> $NEW"
  echo "REMOTE: none"
fi
