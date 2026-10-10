#!/usr/bin/env bash
# git-merge-state.sh — /git-merge の事前確認（対象の検証と、マージの前に要る状態の収集）
#
# Usage: git-merge-state.sh [branch]
#
# branch を省くと今のブランチを対象にする。次のときは ERROR を出して exit 1（マージに進まない）:
#   - 対象がローカルにない、または既定ブランチ（main / master）自身
#   - GitHub リモートがある（/gh-pr-create と /gh-pr-merge の流れで、ここでは扱わない）
#   - 対象の worktree に未コミットの変更がある
#
# 既定ブランチはローカルの main / master（リモートのないリポジトリ向けなので origin は見ない）。
#
# 出力（既定ブランチがなければ COMMITS と BEHIND の代わりに ROOT_COMMITS）:
#   TARGET: <branch>
#   DEFAULT: <branch>|none
#   COMMITS: <n>                  … 既定ブランチにまだ入っていないコミット数
#   BEHIND: <n>                   … 対象にまだ入っていない既定ブランチのコミット数（0 でなければ rebase）
#   ROOT_COMMITS: <hash> ...      … DEFAULT が none のとき、対象のルートコミット
#   STACKED_ON: <branch>,...|none … 対象の下に積まれている、まだマージしていない別のブランチ
#   TARGET_WORKTREE: <path>|none

set -euo pipefail

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "ERROR: git リポジトリではありません" >&2
  exit 1
fi

DEFAULT=""
for b in main master; do
  if git show-ref --verify --quiet "refs/heads/$b"; then DEFAULT="$b"; break; fi
done

TARGET="${1:-$(git branch --show-current)}"
if [[ -z "$TARGET" ]]; then
  echo "ERROR: 対象のブランチを決められません（detached HEAD）。ブランチ名を渡してください" >&2
  exit 1
fi
if ! git show-ref --verify --quiet "refs/heads/$TARGET"; then
  echo "ERROR: ローカルにブランチ $TARGET がありません" >&2
  exit 1
fi
if [[ "$TARGET" == "main" || "$TARGET" == "master" ]]; then
  echo "ERROR: 既定ブランチ $TARGET 自身はマージの対象にしません" >&2
  exit 1
fi
if git remote -v | grep -q 'github\.com'; then
  echo "ERROR: GitHub リモートがあります。/gh-pr-create と /gh-pr-merge の流れを使ってください" >&2
  exit 1
fi

WT_PATH=$(git worktree list --porcelain | awk -v ref="refs/heads/$TARGET" '
  /^worktree / { path = substr($0, 10) }
  /^branch /   { if ($2 == ref) print path }')
if [[ -n "$WT_PATH" && -n "$(git -C "$WT_PATH" status --porcelain)" ]]; then
  echo "ERROR: $WT_PATH に未コミットの変更があります。/git-commit でコミットしてください" >&2
  exit 1
fi

echo "TARGET: $TARGET"
if [[ -z "$DEFAULT" ]]; then
  echo "DEFAULT: none"
  echo "ROOT_COMMITS: $(git rev-list --max-parents=0 --abbrev-commit "$TARGET" | tr '\n' ' ' | sed 's/ $//')"
else
  echo "DEFAULT: $DEFAULT"
  echo "COMMITS: $(git rev-list --count "$DEFAULT..$TARGET")"
  echo "BEHIND: $(git rev-list --count "$TARGET..$DEFAULT")"
fi

# 対象の履歴に先端が含まれ、既定ブランチには入っていない別のブランチ（先端が同じものは積みと見なさない）
STACKED=()
target_tip=$(git rev-parse "refs/heads/$TARGET")
while IFS= read -r b; do
  [[ "$b" == "$TARGET" || "$b" == "$DEFAULT" ]] && continue
  tip=$(git rev-parse "refs/heads/$b")
  [[ "$tip" == "$target_tip" ]] && continue
  git merge-base --is-ancestor "$tip" "$target_tip" || continue
  if [[ -n "$DEFAULT" ]] && git merge-base --is-ancestor "$tip" "$DEFAULT"; then continue; fi
  STACKED+=("$b")
done < <(git for-each-ref --format='%(refname:short)' refs/heads)
if ((${#STACKED[@]} > 0)); then
  echo "STACKED_ON: $(IFS=,; echo "${STACKED[*]}")"
else
  echo "STACKED_ON: none"
fi

echo "TARGET_WORKTREE: ${WT_PATH:-none}"
