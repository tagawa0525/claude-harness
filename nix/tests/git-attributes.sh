#!/usr/bin/env bash
# =============================================================================
# home-manager の構成（homeConfigurations.test）が、配る attributes（../../config/git/attributes）を
# ~/.config/git/attributes に置き、利用者の attributes を奪わないことを検証する（ADR-0105）
# =============================================================================
# 利用者のファイルとの衝突は home-manager の checkLinkTargets が扱う（git-attributes.nix は足さない）ので、その検査を
# 構成の activation から取り出し、差し替えた HOME で流して、止まることを確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./nix/tests/git-attributes.sh

set -euo pipefail

echo "SKIP: RED（git-attributes.nix はまだ無い）"
exit 0

ROOT=$(git rev-parse --show-toplevel)
# shellcheck source=nix/tests/lib.sh
source "$ROOT/nix/tests/lib.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

failures=0
ok() { echo "  ✓ $1"; }
ng() {
  echo "  ✗ $1" >&2
  failures=$((failures + 1))
}

echo "==> home-manager の構成の home-files と checkLinkTargets を取り出す（homeConfigurations.test）"
FILES=$(nix build --no-link --print-out-paths --option warn-dirty false \
  "$ROOT#homeConfigurations.test.config.home-files")
CHECK=$(nix eval --raw --option warn-dirty false \
  "$ROOT#homeConfigurations.test.config.home.activation.checkLinkTargets.data" |
  grep -o '/nix/store/[^ ]*-check-link-targets\.sh' | head -n 1)
if [[ -z "$CHECK" ]]; then
  echo "checkLinkTargets の中に check-link-targets.sh が見つかりません（home-manager の形が変わった）" >&2
  exit 1
fi
PLACED="$FILES/.config/git/attributes"

if cmp -s "$PLACED" "$ROOT/config/git/attributes"; then
  ok "置くファイルが config/git/attributes と同じ"
else
  ng "置くファイルが config/git/attributes と違う: $(cat "$PLACED" 2>&1)"
fi

# check <HOME>: home-manager の衝突の検査を、置く attributes だけについて流す
check() {
  HOME="$1" bash "$CHECK" "$FILES" "$PLACED" 2>&1
}

mkdir -p "$WORK/empty"
if out=$(check "$WORK/empty"); then
  ok "既存のファイルが無ければ、衝突の検査が通る（対照）"
else
  ng "既存のファイルが無いのに、衝突の検査が止まる: $out"
fi

mkdir -p "$WORK/user/.config/git"
echo '*.png binary' >"$WORK/user/.config/git/attributes"
if out=$(check "$WORK/user"); then
  ng "利用者の attributes があるのに、衝突の検査が通る: $out"
elif grep -q "would be clobbered" <<<"$out" &&
  [[ "$(cat "$WORK/user/.config/git/attributes")" == '*.png binary' ]]; then
  ok "利用者の attributes があれば、衝突の検査が止まり、ファイルは残る"
else
  ng "利用者の attributes での止まり方が違う: $out"
fi

# core.attributesFile が別の場所を指すと、git は置いたファイルを読まないので、評価を止める
expr=$(
  cat <<EOF
let
  flake = builtins.getFlake "git+file://$ROOT";
  extended = flake.homeConfigurations.test.extendModules {
    modules = [ { programs.git.settings.core.attributesFile = "/opt/team/attributes"; } ];
  };
in
extended.activationPackage.drvPath
EOF
)
if out=$(nix eval --raw --impure --option warn-dirty false --expr "$expr" 2>&1); then
  ng "core.attributesFile が別の場所なのに、評価が通る: $out"
elif grep -q "/opt/team/attributes" <<<"$out"; then
  ok "core.attributesFile が別の場所なら、評価が止まる"
else
  ng "core.attributesFile での止まり方が違う: $out"
fi

# 置いたファイルを git が読み、.gitattributes の無いリポジトリで CRLF のファイルを LF で index に入れる
swap_home "$WORK/home"
export GIT_CONFIG_NOSYSTEM=1
unset XDG_CONFIG_HOME
mkdir -p "$HOME/.config/git"
cp "$PLACED" "$HOME/.config/git/attributes"
git init -q "$WORK/repo"
printf 'x\r\ny\r\n' >"$WORK/repo/a.txt"
printf 'echo\n' >"$WORK/repo/run.bat"
git -C "$WORK/repo" add a.txt run.bat
eol=$(git -C "$WORK/repo" ls-files --eol a.txt run.bat)
if [[ "$(grep -c '^i/lf ' <<<"$eol")" -eq 2 ]]; then
  ok "CRLF のファイルが LF で index に入る"
else
  ng "index の改行が LF でない: $eol"
fi
rm "$WORK/repo/run.bat"
git -C "$WORK/repo" checkout -- run.bat
if [[ "$(od -An -c "$WORK/repo/run.bat" | tr -d ' \n')" == 'echo\r\n' ]]; then
  ok "*.bat は CRLF で取り出される"
else
  ng "*.bat が CRLF で取り出されない: $(od -An -c "$WORK/repo/run.bat")"
fi

if [[ "$failures" -ne 0 ]]; then
  echo "❌ $failures 件の失敗" >&2
  exit 1
fi
echo "✅ すべて通過"
