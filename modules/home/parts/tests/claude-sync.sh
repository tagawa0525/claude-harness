#!/usr/bin/env bash
# =============================================================================
# claude-sync が、サイズと mtime が同じで中身の違うファイルも上書きすることを検証する
# =============================================================================
# ../../scripts/claude-sync.sh が対象。home-manager の activation は nix store の .claude
# （mtime が 1）から同期するので、~/.claude 側の mtime も 1 になる。語の置き換えのような
# 同じサイズの変更では、サイズと mtime の両方が前回と同じソースが来る。
# このテストは、その状態（両側の mtime 1、同じサイズ、違う中身）を作って同期し、
# CLAUDE.md、skills、scripts が新しい中身になり、commands は既存のファイルを残すことを確かめる。
#
# 使い方（リポジトリルートで実行。rsync が PATH に必要）:
#   ./modules/home/parts/tests/claude-sync.sh

set -euo pipefail

SYNC="$(cd "$(dirname "$0")/../../scripts" && pwd)/claude-sync.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

export HOME="$WORK/home"
mkdir -p "$HOME"
SRC="$WORK/src"
DEST="$HOME/.claude"

PASS=0
FAIL=0

# write <path> <content>: 中身を書き、store のパスと同じく mtime を 1 にする
write() {
  mkdir -p "$(dirname "$1")"
  printf '%s\n' "$2" > "$1"
  touch -d @1 "$1"
}

# check <説明> <path> <期待する中身>
check() {
  local desc="$1" path="$2" want="$3" got
  got=$(cat "$path")
  if [[ "$got" == "$want" ]]; then
    PASS=$((PASS + 1)); echo "✓ $desc"
  else
    FAIL=$((FAIL + 1)); echo "✗ $desc: expected '$want', got '$got'"
  fi
}

write "$SRC/.claude/CLAUDE.md" "ADR-0175"
write "$SRC/.claude/skills/s/SKILL.md" "ADR-0175"
write "$SRC/.claude/scripts/a.sh" "ADR-0175"
write "$SRC/.claude/commands/c.md" "source"
bash "$SYNC" "$SRC" >/dev/null

# 利用者が手で直した commands は残す（--ignore-existing）
write "$DEST/commands/c.md" "user"

# 同じサイズで中身だけ変え、両側の mtime を 1 に揃える（chmod で書き込み権限の付いた ~/.claude 側も含む）
write "$SRC/.claude/CLAUDE.md" "ADR-NNNN"
write "$SRC/.claude/skills/s/SKILL.md" "ADR-NNNN"
write "$SRC/.claude/scripts/a.sh" "ADR-NNNN"
write "$SRC/.claude/commands/c.md" "change"
touch -d @1 "$DEST/CLAUDE.md" "$DEST/skills/s/SKILL.md" "$DEST/scripts/a.sh"
bash "$SYNC" "$SRC" >/dev/null

check "同じサイズと mtime の CLAUDE.md を上書きする" "$DEST/CLAUDE.md" "ADR-NNNN"
check "同じサイズと mtime の skills のファイルを上書きする" "$DEST/skills/s/SKILL.md" "ADR-NNNN"
check "同じサイズと mtime の scripts のファイルを上書きする" "$DEST/scripts/a.sh" "ADR-NNNN"
check "既存の commands のファイルは上書きしない" "$DEST/commands/c.md" "user"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
