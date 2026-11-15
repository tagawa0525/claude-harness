#!/bin/sh
# resume-state.sh — 再開時に、開いている PR・worktree・Slurm のジョブ・docs/issues の issue を 1 つの出力に集める。
#
# Usage: resume-state.sh        （対象のリポジトリの中で実行する）
#
# 出力（節ごとに `== <名前>` の見出し。集められないものは `(なし)` か理由の 1 行。失敗では止めない）:
#   == worktrees   <パス> <ブランチ> [dirty] [session=<Claude-Session>]（先端のコミットの trailer、無ければ省く）
#   == prs         gh pr list の開いている PR（GitHub の URL を持つ remote と gh があるときだけ）
#   == slurm       squeue の自分のジョブ
#   == issues      docs/issues の status: open の issue（blocked-by があれば添える）
#
# AWS の監視など、プロジェクト固有の状態は集めない（そのリポジトリの README か計画に従う）。

echo "== worktrees"
git worktree list --porcelain | awk '
  /^worktree / { path = substr($0, 10) }
  /^branch /   { sub("refs/heads/", "", $2); print path, $2 }
  /^detached/  { print path, "(detached)" }
' | while read -r path branch; do
  line="$path $branch"
  [ -n "$(git -C "$path" status --porcelain 2>/dev/null)" ] && line="$line dirty"
  session=$(git -C "$path" log -1 --format=%B 2>/dev/null | sed -n 's/^Claude-Session: //p' | head -n 1)
  [ -n "$session" ] && line="$line session=$session"
  echo "$line"
done

echo "== prs"
# remote の名前は問わず、GitHub の URL を持つ remote があるときだけ集める
if command -v gh >/dev/null 2>&1 && git remote -v | grep -q 'github\.com'; then
  gh pr list --state open --json number,title,headRefName \
    --template '{{range .}}#{{.number}} {{.headRefName}} {{.title}}{{"\n"}}{{end}}' 2>&1 || echo "(gh pr list に失敗)"
else
  echo "(なし: gh か GitHub の remote が無い)"
fi

echo "== slurm"
if command -v squeue >/dev/null 2>&1; then
  squeue -h -u "$(id -un)" 2>&1 || echo "(squeue に失敗)"
else
  echo "(なし: squeue が無い)"
fi

echo "== issues"
root=$(git rev-parse --show-toplevel) || exit 1
found=0
for f in "$root"/docs/issues/*.md; do
  [ -f "$f" ] || continue
  # 先頭のフロントマター（最初の --- から次の ---）だけを読む
  front=$(awk 'NR == 1 && $0 != "---" { exit } NR > 1 && $0 == "---" { exit } NR > 1 { print }' "$f")
  echo "$front" | grep -Eq '^status: *"?open"?$' || continue
  line="docs/issues/$(basename "$f")"
  blocked=$(echo "$front" | sed -n 's/^blocked-by: *//p')
  [ -n "$blocked" ] && [ "$blocked" != "[]" ] && line="$line blocked-by=$blocked"
  echo "$line"
  found=1
done
[ "$found" = 1 ] || echo "(なし)"
