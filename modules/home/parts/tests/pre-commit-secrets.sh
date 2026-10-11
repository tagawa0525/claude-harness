#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook が、Claude Code のセッションのコミットで、ステージした中身の機密情報を gitleaks で
# 止めることを検証する（ADR-0015）
# =============================================================================
# コミットの時点の index を見るので、同じコマンドの git add と commit -a で足した変更も検査する。漏えいは場所と
# ルール ID だけを出し、値は出さない。ALLOW_SECRET_COMMIT=1 で迂回でき、Claude Code のセッションでなければ
# 検査しない。プロジェクトのローカルの pre-commit があっても、fork でも検査する。
#
# hook のファイルを nix build で作る（hook が store のパスで呼ぶ gitleaks も一緒にビルドされる）。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-secrets.sh

set -euo pipefail

HOST="${1:-r995}"
ROOT=$(git rev-parse --show-toplevel)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
echo "==> pre-commit hook をビルドする（nixosConfigurations.${HOST}）"
cp "$(nix build --no-link --print-out-paths --option warn-dirty false \
  "$ROOT#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".source")" \
  "$HOOKS/pre-commit"
chmod +x "$HOOKS/pre-commit"

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
git config --global core.hooksPath "$HOOKS"
unset CLAUDECODE ALLOW_SECRET_COMMIT

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# gitleaks の aws-access-token に当たる鍵。このファイル自体が検査に当たらないよう、実行時につなぐ
KEY="AKIA""Z3QK7VYH2PLX4N6M"

# new_repo <dir>: feature ブランチに初期コミットを持つ repo を作る（main ガードを避ける）
new_repo() {
  git init -q -b main "$1"
  echo init > "$1/init.txt"
  git -C "$1" add init.txt
  git -C "$1" commit -q -m "chore: init"
  git -C "$1" switch -q -c feat/x
}

# --- 1. 同じコマンドで add した鍵を止め、場所とルール ID を出し、値は出さない -----------------------------
REPO="$WORK/leak"
new_repo "$REPO"
printf 'aws_access_key_id = %s\n' "$KEY" > "$REPO/conf.txt"
if (cd "$REPO" && git add conf.txt && CLAUDECODE=1 git commit -q -m "chore: conf") >"$WORK/out" 2>&1; then
  ng "同じコマンドで add した鍵で止まらない"
else
  if grep -q "conf.txt:1" "$WORK/out" && grep -q "aws-access-token" "$WORK/out" && ! grep -qF "$KEY" "$WORK/out"; then
    ok "同じコマンドで add した鍵で止め、場所とルール ID を出し、値は出さない"
  else
    ng "鍵で止めたが、場所とルール ID の出し方か、値の隠し方が違う"
  fi
fi

# --- 2. 鍵の無いコミットは通す ------------------------------------------------------------------------
REPO="$WORK/clean"
new_repo "$REPO"
echo plain > "$REPO/a.txt"
git -C "$REPO" add a.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: a" >"$WORK/out" 2>&1; then
  ok "鍵の無いコミットは通す"
else
  ng "鍵の無いコミットで止めた"
fi

# --- 3. commit -a で足した変更も見る ------------------------------------------------------------------
printf 'key = %s\n' "$KEY" > "$REPO/a.txt"
if CLAUDECODE=1 git -C "$REPO" commit -q -am "chore: a2" >"$WORK/out" 2>&1; then
  ng "commit -a で足した鍵で止まらない"
elif grep -q "a.txt:1" "$WORK/out" && grep -q "aws-access-token" "$WORK/out"; then
  ok "commit -a で足した鍵で止める"
else
  ng "commit -a で止めたが、鍵の場所とルール ID を出さない"
fi
git -C "$REPO" checkout -q -- a.txt

# --- 4. 迂回と範囲 -------------------------------------------------------------------------------------
REPO="$WORK/escape"
new_repo "$REPO"
printf 'key = %s\n' "$KEY" > "$REPO/b.txt"
git -C "$REPO" add b.txt
if ALLOW_SECRET_COMMIT=1 CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b" >"$WORK/out" 2>&1; then
  ok "ALLOW_SECRET_COMMIT=1 で迂回できる"
else
  ng "ALLOW_SECRET_COMMIT=1 でも止めた"
fi
printf 'key = %s\n' "$KEY" > "$REPO/c.txt"
git -C "$REPO" add c.txt
if git -C "$REPO" commit -q -m "chore: c" >"$WORK/out" 2>&1; then
  ok "Claude Code のセッションでなければ検査しない"
else
  ng "Claude Code のセッションでないのに止めた"
fi

# --- 5. ローカルの pre-commit があっても、fork でも検査する ----------------------------------------------
REPO="$WORK/local"
new_repo "$REPO"
printf '#!/bin/sh\nexit 0\n' > "$REPO/.git/hooks/pre-commit"
chmod +x "$REPO/.git/hooks/pre-commit"
git -C "$REPO" remote add upstream https://example.com/upstream.git
printf 'key = %s\n' "$KEY" > "$REPO/d.txt"
git -C "$REPO" add d.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: d" >"$WORK/out" 2>&1; then
  ng "ローカルの pre-commit がある fork で、鍵で止まらない"
elif grep -q "d.txt:1" "$WORK/out" && grep -q "aws-access-token" "$WORK/out"; then
  ok "ローカルの pre-commit がある fork でも、鍵で止める"
else
  ng "ローカルの pre-commit がある fork で止めたが、鍵の場所とルール ID を出さない"
fi

# --- 6. Markdown の自動修正がステージし直した中身も見る ----------------------------------------------------
# 自動修正は作業ツリーをステージし直す（#282）。ステージしていない部分の鍵も、コミットに入るなら止める
REPO="$WORK/restage"
new_repo "$REPO"
printf '# t\n\nplain\n' > "$REPO/notes.md"
git -C "$REPO" add notes.md
printf '# t\n\nplain\n\nkey = %s\n' "$KEY" > "$REPO/notes.md"
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: notes" >"$WORK/out" 2>&1; then
  if git -C "$REPO" show HEAD:notes.md | grep -qF "$KEY"; then
    ng "Markdown の自動修正がステージし直した鍵を、検査せずにコミットした"
  else
    ok "Markdown の自動修正がステージし直さず、鍵はコミットに入らない"
  fi
else
  ok "Markdown の自動修正がステージし直した鍵でも止める"
fi

# --- 7. gitleaks が失敗したら、漏えいと分けて理由を出して止める ------------------------------------------
REPO="$WORK/broken"
new_repo "$REPO"
printf 'this is not toml [\n' > "$REPO/.gitleaks.toml"
echo plain > "$REPO/e.txt"
git -C "$REPO" add e.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: e" >"$WORK/out" 2>&1; then
  ng "gitleaks が失敗したのにコミットを通した"
elif grep -q "gitleaks が失敗しました" "$WORK/out" && ! grep -q "機密情報らしき内容" "$WORK/out"; then
  ok "gitleaks が失敗したら、漏えいと分けて理由を出して止める"
else
  ng "gitleaks の失敗を、漏えいとして出した"
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
