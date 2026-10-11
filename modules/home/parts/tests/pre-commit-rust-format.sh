#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook の Rust の書式の検査が、ステージした .rs を、そのクレートの edition で見ることを検証する
# =============================================================================
# ../git.nix の pre-commit は core.hooksPath でグローバル配布され、リポジトリ直下から実行される。
# ステージした .rs ごとに最寄りの Cargo.toml を探し、そのパッケージの edition（workspace からの継承を
# 含む）で検査すること、クレートの外と、パッケージの無い Cargo.toml（virtual workspace）の下の .rs は
# edition 2024 で検査することを確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-rust-format.sh

set -euo pipefail

HOST="${1:-r995}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> pre-commit hook をビルドする（nixosConfigurations.${HOST}）"
cp "$(nix build --no-link --print-out-paths --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".source")" \
  "$WORK/pre-commit"
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

printf 'fn main() {\n    println!("x");\n}\n' > "tools/my tool.rs"
git add "tools/my tool.rs"
check 0 "空白を含むパスのクレート外 .rs も 1 つのファイルとして検査される"

# workspace から edition を継承するメンバー（2021 で正しい整形）と、メンバーの外の .rs
mkdir -p ws/a/src ws/scripts
printf '[workspace]\nmembers = ["a"]\n\n[workspace.package]\nedition = "2021"\n' > ws/Cargo.toml
printf '[package]\nname = "a"\nversion = "0.1.0"\nedition.workspace = true\n' > ws/a/Cargo.toml
printf 'use std::io::{stdout, Write};\n\nfn main() {\n    let _ = stdout().flush();\n}\n' > ws/a/src/main.rs
git add ws
check 0 "workspace から継承した edition（2021）で検査される"

printf 'fn main() {\n    println!("x");\n}\n' > ws/scripts/x.rs
git add ws/scripts/x.rs
check 0 "パッケージの無い Cargo.toml の下の整形済み .rs は通る"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
