#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook の Rust フォーマット検査がクレート単位で行われることを検証する
# =============================================================================
# ../git.nix の pre-commit は core.hooksPath でグローバル配布され、リポジトリ直下から実行される。
# 直下に Cargo.toml が無いリポジトリ（このリポジトリもそう）でサブディレクトリのクレートを
# 変更すると、以前はクレートの edition を無視して rustfmt --edition 2024 に落ちていた。
# ステージ済み .rs ごとに最寄りの Cargo.toml を探し、そのクレートの edition で検査すること、
# クレート外の .rs は rustfmt 直接で検査することを確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-rust-format.sh

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
# GitHub リモートなし・CLAUDECODE 未設定: main ガードは対象外にしてフォーマット検査だけを見る

REPO="$WORK/repo"
git init -q -b main "$REPO"
cd "$REPO"
mkdir -p crate2021/src tools
cat > crate2021/Cargo.toml <<'TOML'
[package]
name = "crate2021"
version = "0.1.0"
edition = "2021"
TOML

PASS=0
FAIL=0

# check <期待 0|1> <説明>: ステージ済みの状態で pre-commit を実行し終了コードを比べる
check() {
  local expect="$1" desc="$2" rc=0
  "$WORK/pre-commit" >"$WORK/out" 2>&1 || rc=$?
  if [[ "$rc" == "$expect" ]]; then
    PASS=$((PASS + 1)); echo "✓ $desc"
  else
    FAIL=$((FAIL + 1)); echo "✗ $desc: expected exit $expect, got $rc"; sed 's/^/    /' "$WORK/out"
  fi
  git reset -q
}

# edition 2021 では正しく、2024 のスタイル（use の並び順）では違反になる整形
printf 'use std::io::{stdout, Write};\n\nfn main() {\n    let _ = stdout().flush();\n}\n' > crate2021/src/main.rs
git add crate2021
check 0 "サブディレクトリのクレートは自身の edition（2021）で検査される"

printf 'fn main(){println!("x");}\n' > crate2021/src/main.rs
git add crate2021
check 1 "クレート内の未整形ファイルは止める"

printf 'fn main() {\n    println!("x");\n}\n' > tools/loose.rs
git add tools/loose.rs
check 0 "クレート外の .rs は rustfmt で検査され、整形済みなら通る"

printf 'fn main(){println!("x");}\n' > tools/loose.rs
git add tools/loose.rs
check 1 "クレート外の未整形 .rs は止める"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
