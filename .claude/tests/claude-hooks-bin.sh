# shellcheck shell=bash
# claude-hooks-bin.sh — 中身を claude-hooks script に移したスクリプト（ADR-0087）のテストが、ブランチでビルドした
# claude-hooks を CLAUDE_HOOKS_BIN に入れる（source して使う。lib.sh が HOME を差し替える前に source する。
# ~/.cargo/config.toml の sccache 等を使うため。hooks.sh と同じ）
#
# CLAUDE_HOOKS_BIN（絶対パス）が渡されていればそれを使う。渡されたバイナリが script のサブコマンドを持つかも確かめる
# （前の版を渡すと、移したスクリプトのテストがすべて「名前が無い」で落ち、古さが紛れる）。使えなければ理由を出して
# テストを終える

if [[ -z "${CLAUDE_HOOKS_BIN:-}" ]]; then
  # shellcheck source=cargo-bin.sh
  source "$(dirname "${BASH_SOURCE[0]}")/cargo-bin.sh"
  CLAUDE_HOOKS_BIN=$(cargo_bin "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../modules/home/parts/claude-hooks" && pwd)" claude-hooks) || exit 1
fi
if [[ "$CLAUDE_HOOKS_BIN" != /* ]]; then
  echo "CLAUDE_HOOKS_BIN は絶対パスで渡してください: $CLAUDE_HOOKS_BIN" >&2
  exit 1
fi
# script の使い方の文は終了コード 2 で終わるので、出力を受けてから見る（パイプにすると pipefail で 2 が残る）
claude_hooks_script_usage=$("$CLAUDE_HOOKS_BIN" script 2>&1 </dev/null) || true
if ! grep -q '^scripts: ' <<<"$claude_hooks_script_usage"; then
  echo "claude-hooks が script のサブコマンドを持ちません（古いバイナリ）: $CLAUDE_HOOKS_BIN" >&2
  exit 1
fi
export CLAUDE_HOOKS_BIN
