#!/usr/bin/env bash
# =============================================================================
# グローバルの git hook（pre-commit、pre-merge-commit、commit-msg）が、linked worktree でも
# プロジェクトローカルの .git/hooks/<name> を呼ぶことを検証する
# =============================================================================
# ../git.nix の hook は core.hooksPath でグローバル配布され、プロジェクトローカルの hook があれば呼ぶ。
# ローカルの hook は共通の .git/hooks/ にあり、linked worktree の --git-dir（.git/worktrees/<name>）
# の下には無い。
#
# hook のファイルを nix build で作る。一時 HOME には ~/.claude を置かない。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/local-hooks-worktree.sh

set -euo pipefail

HOST="${1:-r995}"
ROOT=$(git rev-parse --show-toplevel)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
for name in pre-commit pre-merge-commit commit-msg; do
  echo "==> $name hook をビルドする（nixosConfigurations.${HOST}）"
  BUILT=$(nix build --no-link --print-out-paths --option warn-dirty false \
    "$ROOT#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/$name\".source")
  cp "$BUILT" "$HOOKS/$name"
  chmod +x "$HOOKS/$name"
done

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
git config --global core.hooksPath "$HOOKS"
unset CLAUDECODE

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; }

# ローカルの hook は、呼ばれたら共通の .git に名前を書く
REPO="$WORK/repo"
git init -q -b main "$REPO"
echo init >"$REPO/init.txt"
git -C "$REPO" add init.txt
git -C "$REPO" commit -q -m "chore: init"
LOG="$REPO/.git/local-hooks.log"
for name in pre-commit pre-merge-commit commit-msg; do
  printf '#!/bin/sh\necho %s >>"%s"\n' "$name" "$LOG" >"$REPO/.git/hooks/$name"
  chmod +x "$REPO/.git/hooks/$name"
done

TREE="$WORK/tree"
git -C "$REPO" worktree add -q -b feat/x "$TREE"
git -C "$REPO" branch -q feat/y

echo "==> linked worktree でコミットとマージをする"
: >"$LOG"
echo a >"$TREE/a.txt"
git -C "$TREE" add a.txt
git -C "$TREE" commit -q -m "feat: a"
for name in pre-commit commit-msg; do
  if grep -qx "$name" "$LOG"; then
    ok "linked worktree のコミットで、ローカルの $name を呼ぶ"
  else
    ng "linked worktree のコミットで、ローカルの $name を呼ぶ"
  fi
done

: >"$LOG"
git -C "$REPO" switch -q feat/y
echo b >"$REPO/b.txt"
git -C "$REPO" add b.txt
git -C "$REPO" commit -q -m "feat: b"
: >"$LOG"
git -C "$TREE" merge -q --no-ff -m "Merge: b" feat/y
if grep -qx pre-merge-commit "$LOG"; then
  ok "linked worktree のマージで、ローカルの pre-merge-commit を呼ぶ"
else
  ng "linked worktree のマージで、ローカルの pre-merge-commit を呼ぶ"
fi

echo "==> 主の worktree でも呼ぶ"
: >"$LOG"
echo c >"$REPO/c.txt"
git -C "$REPO" add c.txt
git -C "$REPO" commit -q -m "feat: c"
if grep -qx pre-commit "$LOG"; then
  ok "主の worktree のコミットで、ローカルの pre-commit を呼ぶ"
else
  ng "主の worktree のコミットで、ローカルの pre-commit を呼ぶ"
fi

echo
echo "passed: $PASS, failed: $FAIL"
((FAIL == 0))
