#!/usr/bin/env bash
# run-checks.sh — プロジェクトの言語を検出し、フォーマット → リント → テストを実行する
#
# Usage: run-checks.sh
#
# 言語の検出（language-checks スキルの規約。いずれかを満たせば対象）:
#   Rust:     Cargo.toml がある、または .rs がステージ済み
#   Python:   pyproject.toml / setup.py / requirements.txt がある、または .py がステージ済み
#   Nix:      flake.nix がある、または .nix がステージ済み
#   Markdown: .md がステージ済み（自動修正 → 補完 → 再ステージ → 検査）
#
# 最初に失敗したチェックで止まり、失敗したコマンドと自動修正コマンド（あれば）を出す。
# ツールがなければ SKIP を出して続行する。
#
# 出力:
#   RUN: <lang> <stage>: <command>
#   OK: <lang> <stage> | SKIP: <lang> <stage> (<理由>) | FAILED: <lang> <stage>: <command>
#   FIX: <自動修正コマンド>（FAILED のときのみ、あれば）
#   ALL_OK（すべて通ったとき）
#
# プロジェクトの CLAUDE.md にチェックコマンドが明記されていればそちらを優先する
# （例: feature gate のあるクレートでは --all-features が必要）。ここでの既定値は汎用。

set -uo pipefail

ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "ERROR: git リポジトリ内で実行してください" >&2
  exit 1
}
cd "$ROOT" || exit 1

staged_has() {
  git diff --cached --name-only --diff-filter=ACM -- "$1" | grep -q .
}

# run_stage <lang> <stage> <tool> <fix-command|""> <command...>
# command は関数名でもよい（パイプラインを含むチェック用）
run_stage() {
  local lang="$1" stage="$2" tool="$3" fix="$4"
  shift 4
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "SKIP: $lang $stage ($tool not found)"
    return 0
  fi
  echo "RUN: $lang $stage: $*"
  if "$@"; then
    echo "OK: $lang $stage"
  else
    echo "FAILED: $lang $stage: $*"
    [[ -n "$fix" ]] && echo "FIX: $fix"
    exit 1
  fi
}

DETECTED=0

# --- Rust ---
if [[ -f Cargo.toml ]] || staged_has '*.rs'; then
  DETECTED=1
  run_stage rust format cargo "cargo fmt" cargo fmt --check
  run_stage rust lint cargo "" cargo clippy --all-targets -- -D warnings
  run_stage rust test cargo "" cargo test
fi

# --- Python ---
if [[ -f pyproject.toml || -f setup.py || -f requirements.txt ]] || staged_has '*.py'; then
  DETECTED=1
  run_stage python format ruff "ruff format ." ruff format --check .
  run_stage python lint ruff "ruff check --fix ." ruff check .
  if [[ -d tests ]] || compgen -G 'test_*.py' >/dev/null || compgen -G '*_test.py' >/dev/null; then
    run_stage python test pytest "" pytest
  else
    echo "SKIP: python test (テストが見つからない)"
  fi
fi

# --- Nix ---
nix_fmt_check() {
  git ls-files -z '*.nix' | xargs -0 -r nixfmt --check
}
if [[ -f flake.nix ]] || staged_has '*.nix'; then
  DETECTED=1
  run_stage nix format nixfmt "git ls-files -z '*.nix' | xargs -0 -r nixfmt" nix_fmt_check
  run_stage nix lint statix "statix fix" statix check
  if [[ -f flake.nix ]]; then
    run_stage nix test nix "" nix flake check
  else
    echo "SKIP: nix test (flake.nix がない)"
  fi
fi

# --- Markdown（ステージ済みのみ）---
MD_FIXER="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/fix-markdown-lint.py"
staged_md() {
  git diff --cached --name-only -z --diff-filter=ACM -- '*.md'
}
md_autofix() {
  staged_md | xargs -0 -r markdownlint --fix -- 2>/dev/null || true
  staged_md | xargs -0 -r python3 "$MD_FIXER" || return 1
  staged_md | xargs -0 -r git add --
}
md_lint() {
  staged_md | xargs -0 -r markdownlint --
}
if staged_has '*.md'; then
  DETECTED=1
  run_stage markdown format markdownlint "" md_autofix
  run_stage markdown lint markdownlint "" md_lint
fi

if (( DETECTED == 0 )); then
  echo "NOTE: 対象言語が見つかりません（Cargo.toml / pyproject.toml / flake.nix、またはステージ済みの .rs / .py / .nix / .md）"
  exit 0
fi

echo "ALL_OK"
