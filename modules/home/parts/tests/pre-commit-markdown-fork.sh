#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook の Markdown 自動修正が fork（upstream リモートのある clone）では
# 飛ばされることを検証する
# =============================================================================
# ../git.nix の pre-commit は core.hooksPath でグローバル配布され、ステージ済みの .md に
# markdownlint --fix と fix-markdown-lint.py を当てる。他人のプロジェクトの fork（上流に PR を
# 出す作業木。`upstream` リモートを持つ）でこれが走ると、こちらの規約を上流の文書に押し付け、
# 無関係な差分（``` → ```text 等）を上流向けのコミットに混ぜてしまう。
# upstream リモートのない repo では従来どおり自動修正され（末尾の空白が落ちる）、ある repo では
# ステージ済みの内容が 1 バイトも変わらないことを確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-markdown-fork.sh

set -euo pipefail

HOST="${1:-r995}"

if ! command -v markdownlint >/dev/null 2>&1; then
  echo "✗ markdownlint が PATH にない。自動修正の有無を比べられないので、このテストは成立しない"
  exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> pre-commit hook を取り出す（nixosConfigurations.${HOST}）"
nix eval --raw --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".text" \
  > "$WORK/pre-commit"
chmod +x "$WORK/pre-commit"

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
# GitHub リモートなし・CLAUDECODE 未設定: main ガードは対象外にして Markdown 段だけを見る

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# 末尾の空白 3 つ（MD009。2 つは改行の意味で許される）は markdownlint --fix 単独で直る違反。
# HOME を退避した環境には fix-markdown-lint.py（~/.claude 配下）が無いので、それに頼る違反
# （言語指定のないフェンス）は使わない
ORIGINAL=$'# Title\n\nSome text.   \n'

# run_hook <repo>: ステージ済みの状態で pre-commit を実行し、終了コードを返す
run_hook() {
  local rc=0
  (cd "$1" && "$WORK/pre-commit") >"$WORK/out" 2>&1 || rc=$?
  return $rc
}

# --- 1. upstream リモートのない repo: 従来どおり自動修正される ---------------------------
PLAIN="$WORK/plain"
git init -q -b main "$PLAIN"
printf '%s' "$ORIGINAL" > "$PLAIN/README.md"
git -C "$PLAIN" add README.md
if run_hook "$PLAIN"; then
  staged=$(git -C "$PLAIN" show :README.md; printf x); staged=${staged%x}
  if [[ "$staged" != "$ORIGINAL" ]]; then
    ok "upstream リモートのない repo では Markdown が自動修正されステージし直される"
  else
    ng "upstream リモートのない repo なのにステージ済みの README.md が変わっていない"
  fi
else
  ng "upstream リモートのない repo でフックが失敗した"
fi

# --- 2. upstream リモートのある repo（fork）: 触られない ---------------------------------
FORK="$WORK/fork"
git init -q -b main "$FORK"
git -C "$FORK" remote add upstream https://example.invalid/upstream.git
printf '%s' "$ORIGINAL" > "$FORK/README.md"
git -C "$FORK" add README.md
if run_hook "$FORK"; then
  staged=$(git -C "$FORK" show :README.md; printf x); staged=${staged%x}
  if [[ "$staged" == "$ORIGINAL" ]]; then
    ok "upstream リモートのある fork ではステージ済みの README.md が 1 バイトも変わらない"
  else
    ng "fork なのにステージ済みの README.md が書き換えられた"
  fi
  if grep -q "飛ばします" "$WORK/out"; then
    ok "fork では Markdown 段を飛ばした旨を出力する"
  else
    ng "fork で飛ばした旨の出力がない"
  fi
else
  ng "fork でフックが失敗した（上流の文書の違反で止めてはならない）"
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
