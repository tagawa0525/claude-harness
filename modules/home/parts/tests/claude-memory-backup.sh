#!/usr/bin/env bash
# =============================================================================
# claude-memory-backup が、自動メモリだけをブランチ memory-backup にコミットすることを検証する
# =============================================================================
# ../../scripts/claude-memory-backup.sh が対象（Claude Code の Stop の hook。ADR-0076）。
# 一時のディレクトリに ~/.claude に見立てたリポジトリ（ブランチ main）を作り、次を確かめる:
#   - projects/*/memory/ の下のファイルだけが memory-backup のツリーに入る
#   - 中身が変わらなければコミットを足さず、変われば前のコミットを親にして足す
#   - 消したメモリは次のツリーから消え、前のコミットから戻せる
#   - main の HEAD、index（ステージした変更）、作業ツリーは変わらない
#   - git の hook（グローバルに配る pre-commit、commit-msg など）がどれも失敗する設定でも、CLAUDECODE=1 でもコミットできる
#   - 同時に呼んでも、どちらの変更も失わずに 1 本の履歴になる
#   - リポジトリでないディレクトリでは、1 行目に理由を書いて 0 でない終了コードで終わる
#
# 使い方（リポジトリルートで実行。git、flock が PATH に必要）:
#   ./modules/home/parts/tests/claude-memory-backup.sh

set -euo pipefail

BACKUP="$(cd "$(dirname "$0")/../../scripts" && pwd)/claude-memory-backup.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$WORK/gitconfig"
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
REF=refs/heads/memory-backup

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; }

# check <説明> <期待する値> <実際の値>
check() {
  if [[ "$3" == "$2" ]]; then ok "$1"; else ng "$1: expected '$2', got '$3'"; fi
}

# run <claude-dir> [stdin の JSON]: hook と同じく標準入力に JSON を渡して呼ぶ
run() {
  printf '%s' "${2:-{\}}" | bash "$BACKUP" "$1"
}

CD="$WORK/claude"
mkdir -p "$CD/projects/-a/memory" "$CD/projects/-b/memory" "$CD/projects/-a/sessions"
git -C "$CD" init -q
printf 'settings\n' >"$CD/settings.json"
git -C "$CD" add settings.json
git -C "$CD" commit -q -m init
printf 'index a\n' >"$CD/projects/-a/memory/MEMORY.md"
printf 'note b\n' >"$CD/projects/-b/memory/note.md"
printf 'log\n' >"$CD/projects/-a/session.jsonl"
printf 'x\n' >"$CD/projects/-a/sessions/s.json"
printf 'top\n' >"$CD/other.txt"
# 利用者がステージした変更（main の index）は残す
printf 'settings2\n' >"$CD/settings.json"
git -C "$CD" add settings.json
HEAD0=$(git -C "$CD" rev-parse HEAD)
INDEX0=$(git -C "$CD" diff --cached --name-status)

if run "$CD" '{"session_id":"s1","cwd":"/w"}'; then ok "1 回目が成功する"; else ng "1 回目が失敗した"; fi
check "ツリーはメモリのファイルだけ" \
  "$(printf 'projects/-a/memory/MEMORY.md\nprojects/-b/memory/note.md')" \
  "$(git -C "$CD" ls-tree -r --name-only "$REF" 2>&1)"
check "最初のコミットは親を持たない" "1" "$(git -C "$CD" rev-list --count "$REF" 2>&1)"
check "本文に session_id が入る" "1" "$(git -C "$CD" log -1 --format=%B "$REF" 2>/dev/null | grep -c 's1')"
check "HEAD は変わらない" "$HEAD0" "$(git -C "$CD" rev-parse HEAD)"
check "ブランチは main のまま" "main" "$(git -C "$CD" branch --show-current)"
check "ステージした変更は残る" "$INDEX0" "$(git -C "$CD" diff --cached --name-status)"

C1=$(git -C "$CD" rev-parse -q --verify "$REF" || true)
run "$CD" || true
check "中身が同じならコミットを足さない" "$C1" "$(git -C "$CD" rev-parse -q --verify "$REF" || true)"

printf 'index a2\n' >"$CD/projects/-a/memory/MEMORY.md"
run "$CD" || true
check "変わったら前のコミットを親にして足す" "$C1" "$(git -C "$CD" rev-parse -q --verify "$REF^" 2>/dev/null || true)"
check "新しい中身が入る" "index a2" "$(git -C "$CD" show "$REF:projects/-a/memory/MEMORY.md" 2>&1)"

rm "$CD/projects/-b/memory/note.md"
run "$CD" || true
check "消したメモリは次のツリーから消える" "projects/-a/memory/MEMORY.md" \
  "$(git -C "$CD" ls-tree -r --name-only "$REF" 2>&1)"
check "消したメモリは前のコミットから戻せる" "note b" \
  "$(git -C "$CD" show "$REF~1:projects/-b/memory/note.md" 2>&1)"

# git の hook がどれも失敗し、Claude Code のセッションの印がある形でも止まらない
HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
for h in pre-commit commit-msg prepare-commit-msg post-commit pre-merge-commit post-rewrite; do
  printf '#!/bin/sh\nexit 1\n' >"$HOOKS/$h"
  chmod +x "$HOOKS/$h"
done
git -C "$CD" config core.hooksPath "$HOOKS"
printf 'index a3\n' >"$CD/projects/-a/memory/MEMORY.md"
if CLAUDECODE=1 run "$CD"; then ok "失敗する hook と CLAUDECODE=1 のもとでも成功する"; else ng "失敗する hook のもとで失敗した"; fi
check "hook のもとでも中身が入る" "index a3" "$(git -C "$CD" show "$REF:projects/-a/memory/MEMORY.md" 2>&1)"
git -C "$CD" config --unset core.hooksPath

# 同時に呼ぶ: 各プロセスの前に別のファイルを足し、最後のツリーに全部が残るかを見る
for i in 1 2 3 4 5 6; do
  printf 'p%s\n' "$i" >"$CD/projects/-a/memory/p$i.md"
  run "$CD" &
done
wait
run "$CD" || true
check "同時に呼んでも全部のファイルが残る" "6" \
  "$(git -C "$CD" ls-tree -r --name-only "$REF" 2>/dev/null | grep -c '/p[0-9]\.md$')"
check "履歴は 1 本（マージを作らない）" "0" "$(git -C "$CD" rev-list --merges --count "$REF" 2>&1)"
check "一時の index とロックのファイルを残さない" "" \
  "$(find "$(git -C "$CD" rev-parse --absolute-git-dir)" -maxdepth 1 -name 'memory-backup*' ! -name '*.lock' 2>&1)"
check "HEAD は最後まで変わらない" "$HEAD0" "$(git -C "$CD" rev-parse HEAD)"
check "ステージした変更は最後まで残る" "$INDEX0" "$(git -C "$CD" diff --cached --name-status)"

# メモリの無いリポジトリでは、ブランチを作らずに成功する
EMPTY="$WORK/empty"
mkdir -p "$EMPTY"
git -C "$EMPTY" init -q
if run "$EMPTY"; then ok "メモリが無ければ成功する"; else ng "メモリが無いと失敗した"; fi
check "メモリが無ければブランチを作らない" "" "$(git -C "$EMPTY" rev-parse -q --verify "$REF" || true)"

# リポジトリでないディレクトリは、理由を 1 行目に書いて失敗する（Claude Code は 1 行目を通知に出す）
NOREPO="$WORK/norepo"
mkdir -p "$NOREPO/projects/-a/memory"
printf 'x\n' >"$NOREPO/projects/-a/memory/MEMORY.md"
if run "$NOREPO" 2>"$WORK/err"; then
  ng "リポジトリでないのに成功した"
else
  check "失敗の 1 行目はこのスクリプトの名前で始まる" "claude-memory-backup:" "$(head -1 "$WORK/err" | cut -d' ' -f1)"
fi

# ~/.claude を省いたときは HOME の下を使う
DEF="$HOME/.claude"
mkdir -p "$DEF/projects/-a/memory"
git -C "$DEF" init -q
printf 'def\n' >"$DEF/projects/-a/memory/MEMORY.md"
printf '{}' | bash "$BACKUP" || true
check "引数を省くと ~/.claude を使う" "def" "$(git -C "$DEF" show "$REF:projects/-a/memory/MEMORY.md" 2>&1)"

echo
echo "pass: $PASS, fail: $FAIL"
[ "$FAIL" -eq 0 ]
