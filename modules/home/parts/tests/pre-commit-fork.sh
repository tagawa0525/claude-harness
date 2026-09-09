#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook のこちらの規約の検査が fork（upstream リモートのある clone）では
# 飛ばされることを検証する
# =============================================================================
# ../git.nix の pre-commit は core.hooksPath でグローバル配布され、ステージ済みのファイルに
# こちらの道具の版と規約（ruff、markdownlint --fix と fix-markdown-lint.py、rustfmt）を当てる。
# 他人のプロジェクトの fork（上流に PR を出す作業木。`upstream` リモートを持つ）でこれが走ると、
# 新しい ruff が触っていない上流の行を上流の版にないルール（RUF043 等）で落とし、Markdown の
# 自動修正が無関係な差分（``` → ```text 等）を上流向けのコミットに混ぜてしまう。
# upstream リモートのない repo では従来どおり検査され（Markdown は自動修正され、lint に落ちる
# Python は止められる）、ある repo では検査されずステージ済みの内容が 1 バイトも変わらないことを
# 確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-fork.sh

set -euo pipefail

HOST="${1:-r995}"

if ! command -v markdownlint >/dev/null 2>&1; then
  echo "✗ markdownlint が PATH にない。自動修正の有無を比べられないので、このテストは成立しない"
  exit 1
fi
if ! command -v ruff >/dev/null 2>&1; then
  echo "✗ ruff が PATH にない。Python の検査の有無を比べられないので、このテストは成立しない"
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
# GitHub リモートなし・CLAUDECODE 未設定: main ガードは対象外にして規約の検査だけを見る

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# 末尾の空白 3 つ（MD009。2 つは改行の意味で許される）は markdownlint --fix 単独で直る違反。
# HOME を退避した環境には fix-markdown-lint.py（~/.claude 配下）が無いので、それに頼る違反
# （言語指定のないフェンス）は使わない
ORIGINAL=$'# Title\n\nSome text.   \n'
# 使わない import（F401）はどの版の ruff でも既定で落ちる違反
BAD_PY=$'import os\n'

# run_hook <repo>: ステージ済みの状態で pre-commit を実行し、終了コードを返す
run_hook() {
  local rc=0
  (cd "$1" && "$WORK/pre-commit") >"$WORK/out" 2>&1 || rc=$?
  return $rc
}

# --- 1. upstream リモートのない repo: Markdown は従来どおり自動修正される ---------------------
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

# --- 2. upstream リモートのない repo: lint に落ちる Python は止められる ----------------------
printf '%s' "$BAD_PY" > "$PLAIN/bad.py"
git -C "$PLAIN" add bad.py
if run_hook "$PLAIN"; then
  ng "upstream リモートのない repo で lint に落ちる Python が通った（fork の比較が成立しない）"
else
  ok "upstream リモートのない repo では lint に落ちる Python が止められる"
fi

# --- 3. upstream リモートのある repo（fork）: 何も検査せず、触られない -------------------------
FORK="$WORK/fork"
git init -q -b main "$FORK"
git -C "$FORK" remote add upstream https://example.invalid/upstream.git
printf '%s' "$ORIGINAL" > "$FORK/README.md"
printf '%s' "$BAD_PY" > "$FORK/bad.py"
git -C "$FORK" add README.md bad.py
if run_hook "$FORK"; then
  staged=$(git -C "$FORK" show :README.md; printf x); staged=${staged%x}
  if [[ "$staged" == "$ORIGINAL" ]]; then
    ok "upstream リモートのある fork ではステージ済みの README.md が 1 バイトも変わらない"
  else
    ng "fork なのにステージ済みの README.md が書き換えられた"
  fi
  staged_py=$(git -C "$FORK" show :bad.py; printf x); staged_py=${staged_py%x}
  if [[ "$staged_py" == "$BAD_PY" ]]; then
    ok "fork ではステージ済みの bad.py も 1 バイトも変わらない（将来の自動修正の見逃しを防ぐ）"
  else
    ng "fork なのにステージ済みの bad.py が書き換えられた"
  fi
  if grep -q "飛ばします" "$WORK/out"; then
    ok "fork では検査を飛ばした旨を出力する"
  else
    ng "fork で飛ばした旨の出力がない"
  fi
else
  ng "fork でフックが失敗した（上流の行の違反で止めてはならない）"
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
