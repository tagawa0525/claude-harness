#!/usr/bin/env bash
# =============================================================================
# スクリプトが生成するコミットメッセージが commit-msg hook を通ることを検証する
# =============================================================================
# ../git.nix の commit-msg hook（Conventional Commits を強制）と、リポジトリ内で
# 自動コミットするスクリプトの整合を見る。
#
# 実際に起きた不整合: nix-rebuild.sh が `flake: update (host)` を生成していたが、
# hook の型リストに `flake` がないため Claude Code セッション（CLAUDECODE=1）からの
# update が最後のコミットで必ず失敗していた。片方だけ変えると再発するため、
# 「スクリプトが書くメッセージ」を hook 自身に通して確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/commit-msg-conventions.sh
#
# hook の本体は nix の設定内にインラインで書かれているため、nix eval で取り出す。

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

HOST="${1:-r995}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> commit-msg hook を取り出す（nixosConfigurations.${HOST}）"
nix eval --raw --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/commit-msg\".text" \
  > "$WORK/commit-msg"
chmod +x "$WORK/commit-msg"

# hook は Claude Code セッションのコミットにだけ効く
export CLAUDECODE=1
# hook は git リポジトリ外だと何もせず exit 0 するため、一時リポジトリの中で実行する
# （このリポジトリ自身の .git/hooks へ委譲されないよう、新規 init したものを使う）
git init -q "$WORK/repo"
cd "$WORK/repo"

PASS=0
FAIL=0

# check <期待 ok|ng> <出所> <メッセージ>
check() {
  local expect="$1" origin="$2" msg="$3"
  printf '%s\n' "$msg" > "$WORK/msg"
  if "$WORK/commit-msg" "$WORK/msg" >"$WORK/out" 2>&1; then
    local actual=ok
  else
    local actual=ng
  fi
  if [[ "$actual" == "$expect" ]]; then
    PASS=$((PASS + 1))
    echo "✓ ${origin}: ${msg}"
  else
    FAIL=$((FAIL + 1))
    echo "✗ ${origin}: ${msg}" >&2
    echo "    expected ${expect}, got ${actual}" >&2
    sed 's/^/    /' "$WORK/out" >&2
  fi
}

echo "==> スクリプトが生成するメッセージを収集して検証する"
# `git commit -m "..."` の形で書かれたリテラルのメッセージを集める。
# 変数展開（$HOSTNAME 等）はダミー値に置き換えてから検証する
found=0
while IFS= read -r line; do
  msg=$(sed -E 's/.*git commit[^"]*"([^"]*)".*/\1/' <<<"$line")
  [[ -n "$msg" ]] || continue
  msg="${msg//\$HOSTNAME/testhost}"
  msg="${msg//\$\{HOSTNAME\}/testhost}"
  found=$((found + 1))
  check ok "modules/home/scripts" "$msg"
done < <(grep -rhE 'git commit +-m +"[^"]+"' "$ROOT/modules/home/scripts" || true)

if (( found == 0 )); then
  echo "✗ スクリプトから git commit -m のメッセージを 1 件も抽出できませんでした" >&2
  echo "   （書き方が変わった可能性がある。抽出パターンを見直すこと）" >&2
  FAIL=$((FAIL + 1))
fi

echo "==> hook 自体の判定（回帰確認）"
check ok "sanity" "feat(scope): 追加する"
check ng "sanity" "flake: update (testhost)"
check ng "sanity" "何の型もない件名"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
