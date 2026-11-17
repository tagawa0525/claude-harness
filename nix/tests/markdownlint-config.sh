#!/usr/bin/env bash
# =============================================================================
# 配る markdownlint のグローバル設定（../../../../config/markdownlint/config）を、markdownlint-cli が
# ~/.config/markdownlint/config から読み、規則が効くことを検証する（ADR-0093）
# =============================================================================
# 読み込みの rc は、{ で始まらない内容を JSON でなく ini として読み、設定が黙って効かなくなる。設定の先頭にコメントを
# 足すだけで起きるので、配る設定そのものを markdownlint に読ませて確かめる。設定の無い対照も動かし、この
# 検査が設定の有無を見分けられることを示す。
#
# markdownlint は homeConfigurations.test の home.path から取る（git-hooks.nix が入れる）。
#
# 使い方（リポジトリルートで実行）:
#   ./nix/tests/markdownlint-config.sh

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> markdownlint をビルドする（homeConfigurations.test）"
MARKDOWNLINT="$(nix build --no-link --print-out-paths --option warn-dirty false \
  "$ROOT#homeConfigurations.test.config.home.path")/bin/markdownlint"

failures=0
ok() { echo "  ✓ $1"; }
ng() {
  echo "  ✗ $1" >&2
  failures=$((failures + 1))
}

# 既定の規則が止める 3 つ（長い行、インライン HTML、URL の直書き）を持つ文書
mkdir -p "$WORK/cwd"
{
  echo '# T'
  echo
  echo '<b>x</b>'
  echo
  echo 'http://a.example'
  echo
  for _ in $(seq 40); do printf 'word '; done
  echo
} >"$WORK/cwd/a.md"

# lint <HOME>: 設定の無いディレクトリで実行し、出力を返す（違反があれば終了コードは 1 なので無視する）
lint() {
  (cd "$WORK/cwd" && HOME="$1" "$MARKDOWNLINT" -- a.md 2>&1 || true)
}

mkdir -p "$WORK/bare"
bare=$(lint "$WORK/bare")
for rule in MD013 MD033 MD034; do
  if grep -q "$rule" <<<"$bare"; then
    ok "設定が無いと $rule を指摘する（対照）"
  else
    ng "設定が無いのに $rule を指摘しない。この検査は設定の有無を見分けられない: $bare"
  fi
done

mkdir -p "$WORK/configured/.config/markdownlint"
cp "$ROOT/config/markdownlint/config" "$WORK/configured/.config/markdownlint/config"
configured=$(lint "$WORK/configured")
for rule in MD013 MD033 MD034; do
  if grep -q "$rule" <<<"$configured"; then
    ng "配る設定を ~/.config/markdownlint/config に置いても $rule を指摘する（設定が読まれていない）: $configured"
  else
    ok "配る設定が $rule を無効にする"
  fi
done
# 設定が読まれても、無効にしていない規則は効く（default: true）
if grep -q MD009 <<<"$configured"; then
  ok "無効にしていない規則（MD009）は効く"
else
  ng "無効にしていない規則 MD009 を指摘しない: $configured"
fi

# front matter の title と # の見出しを両方持つ文書（docs/ の文書）は、title を先頭の見出しに数えない設定で通る
{
  echo '---'
  echo 'title: T'
  echo '---'
  echo
  echo '# T'
} >"$WORK/cwd/fm.md"
lint_fm() {
  (cd "$WORK/cwd" && HOME="$1" "$MARKDOWNLINT" -- fm.md 2>&1 || true)
}
if grep -q MD025 <<<"$(lint_fm "$WORK/bare")"; then
  ok "設定が無いと、front matter の title と # の見出しを MD025 が指摘する（対照）"
else
  ng "設定が無いのに MD025 を指摘しない。この検査は設定の有無を見分けられない"
fi
if grep -q MD025 <<<"$(lint_fm "$WORK/configured")"; then
  ng "配る設定を置いても、front matter の title と # の見出しを MD025 が指摘する"
else
  ok "配る設定が、front matter の title を先頭の見出しに数えない"
fi

if ((failures > 0)); then
  echo "FAILED: $failures" >&2
  exit 1
fi
echo "OK"
