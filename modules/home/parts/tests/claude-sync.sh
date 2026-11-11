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
  # ファイルが無いときも set -e で止めず、✗ に数えて残りの検査を続ける
  got=$(cat "$path" 2>/dev/null) || got="(ファイルが無い)"
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

# 同じサイズで中身だけ変える。~/.claude 側は 1 回目の rsync -a で mtime 1 になっている（chmod は mtime を変えない）
write "$SRC/.claude/CLAUDE.md" "ADR-NNNN"
write "$SRC/.claude/skills/s/SKILL.md" "ADR-NNNN"
write "$SRC/.claude/scripts/a.sh" "ADR-NNNN"
write "$SRC/.claude/commands/c.md" "change"
bash "$SYNC" "$SRC" >/dev/null

check "同じサイズと mtime の CLAUDE.md を上書きする" "$DEST/CLAUDE.md" "ADR-NNNN"
check "同じサイズと mtime の skills のファイルを上書きする" "$DEST/skills/s/SKILL.md" "ADR-NNNN"
check "同じサイズと mtime の scripts のファイルを上書きする" "$DEST/scripts/a.sh" "ADR-NNNN"
check "既存の commands のファイルは上書きしない" "$DEST/commands/c.md" "user"

# 引数なしでは、ハーネスの clone から同期する。nixfiles の .claude/CLAUDE.md は nixfiles のプロジェクトの指示で、
# グローバルの指示（~/.claude/CLAUDE.md）に入れると、どのリポジトリのセッションもそれを読む
write "$HOME/nix/claude-harness/.claude/CLAUDE.md" "harness"
write "$HOME/nix/nixfiles/.claude/CLAUDE.md" "nixfiles project"
rc=0
bash "$SYNC" >/dev/null || rc=$?
if (( rc == 0 )); then PASS=$((PASS + 1)); echo "✓ 引数なしの同期が成功する"; else FAIL=$((FAIL + 1)); echo "✗ 引数なしの同期が失敗した（終了コード $rc）"; fi
check "引数なしでは ~/nix/claude-harness から同期する（nixfiles のプロジェクトの指示は入れない）" "$DEST/CLAUDE.md" "harness"

# 引数なしでは、同期の前に既定の clone を git pull --ff-only で最新にする。clone が古いまま同期すると、rebuild で配られた
# 新しい版を、黙って古い版で上書きする（2026-11-12 に、2 台の clone が 1 コミット遅れていた）
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
CLONE="$HOME/nix/claude-harness"
rm -rf "$CLONE"
git init -q --bare "$WORK/origin.git"
git clone -q "$WORK/origin.git" "$CLONE" 2>/dev/null
# advance <clone> <中身>: その clone で CLAUDE.md を書き換えてコミットし、origin に push する
advance() {
  mkdir -p "$1/.claude"
  printf '%s\n' "$2" > "$1/.claude/CLAUDE.md"
  git -C "$1" add .claude/CLAUDE.md
  git -C "$1" commit -qm "docs: $2"
  git -C "$1" push -q origin main
}
advance "$CLONE" "v1"
git clone -q "$WORK/origin.git" "$WORK/other"
advance "$WORK/other" "v2"

rc=0
bash "$SYNC" >"$WORK/out" 2>&1 || rc=$?
if (( rc == 0 )); then PASS=$((PASS + 1)); echo "✓ 引数なしの同期（pull つき）が成功する"; else FAIL=$((FAIL + 1)); echo "✗ 引数なしの同期（pull つき）が失敗した（終了コード $rc）"; sed 's/^/    /' "$WORK/out"; fi
check "引数なしでは、既定の clone を pull してから同期する" "$DEST/CLAUDE.md" "v2"

# パスを渡したときは pull しない（worktree で直しを試す、rebuild の activation、pull できないときに今の中身を入れる）
advance "$WORK/other" "v3"
bash "$SYNC" "$CLONE" >/dev/null
check "パスを渡したときは pull せず、その場所の中身を同期する" "$DEST/CLAUDE.md" "v2"

# pull できないとき（手元と origin の履歴が分かれた）は、古い中身を同期せずに止める
printf 'local\n' > "$CLONE/.claude/CLAUDE.md"
git -C "$CLONE" commit -qam "docs: local"
rc=0
bash "$SYNC" >"$WORK/out" 2>&1 || rc=$?
if (( rc != 0 )) && grep -q "pull" "$WORK/out" && grep -qF "claude-sync $CLONE" "$WORK/out"; then
  PASS=$((PASS + 1)); echo "✓ pull できないときは失敗終了し、pull せずに同期する方法を示す"
else
  FAIL=$((FAIL + 1)); echo "✗ pull できないときは失敗終了し、pull せずに同期する方法を示す（終了コード $rc）"; sed 's/^/    /' "$WORK/out"
fi
check "pull できないときは同期しない" "$DEST/CLAUDE.md" "v2"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
