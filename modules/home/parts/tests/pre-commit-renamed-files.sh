#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook が rename したファイルも検査することを検証する
# =============================================================================
# ../git.nix の pre-commit はステージ済みファイルを git diff --cached で取り出す。
# rename は R になるので、--diff-filter=ACM では old.nix → new.nix のような rename が
# 検査から漏れていた（pathspec なしの一覧では拡張子が変わる rename も R になり、
# rename だけのコミットは「ステージ済みファイルなし」として全検査を飛ばしていた）。
# 未整形のファイルを rename だけでステージし、各言語の検査で止まることを確かめる。
#
# 使い方（リポジトリルートで実行。nixfmt / ruff / markdownlint / cargo が PATH に必要）:
#   ./modules/home/parts/tests/pre-commit-renamed-files.sh

set -euo pipefail

HOST="${1:-r995}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> pre-commit hook を取り出す（nixosConfigurations.${HOST}）"
nix eval --raw --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".text" \
  > "$WORK/pre-commit"
chmod +x "$WORK/pre-commit"

# rustup / cargo は実環境のツールチェインを使う（HOME を退避すると既定のツールチェインが見えなくなる）
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
# GitHub リモートなし・CLAUDECODE 未設定: main ガードは対象外にして各言語の検査だけを見る

REPO="$WORK/repo"
git init -q -b main "$REPO"
cd "$REPO"

PASS=0
FAIL=0

# check_rename <old> <new> <説明>: 未整形の <old> をコミット済みにし、<new> へ rename した
# だけの状態で pre-commit を実行して、止まる（exit 1）ことを確かめる
check_rename() {
  local old="$1" new="$2" desc="$3" rc=0
  git add -- "$old"
  git commit -q --no-verify -m "chore: add $old"
  git mv -- "$old" "$new"
  "$WORK/pre-commit" >"$WORK/out" 2>&1 || rc=$?
  if [[ "$rc" == 1 ]]; then
    PASS=$((PASS + 1)); echo "✓ $desc"
  else
    FAIL=$((FAIL + 1)); echo "✗ $desc: expected exit 1, got $rc"; sed 's/^/    /' "$WORK/out"
  fi
  git commit -q --no-verify -m "chore: rename $old"
}

printf '{a=1;}\n' > old.nix
check_rename old.nix new.nix "rename した未整形の .nix を nixfmt で止める"

printf 'x=1\n' > old.py
check_rename old.py new.py "rename した未整形の .py を ruff で止める"

# トップレベル見出しの重複（MD025）は markdownlint --fix でも直らない
printf '# a\n\n# b\n' > old.md
check_rename old.md new.md "rename した違反のある .md を markdownlint で止める"

mkdir -p crate/src
printf '[package]\nname = "crate"\nversion = "0.1.0"\nedition = "2021"\n' > crate/Cargo.toml
git add crate/Cargo.toml
printf 'fn main(){println!("x");}\n' > crate/src/old.rs
check_rename crate/src/old.rs crate/src/main.rs "rename した未整形の .rs を cargo fmt で止める"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
