#!/usr/bin/env bash
# =============================================================================
# Claude Code Config Sync
# =============================================================================
# リポジトリの .claude（CLAUDE.md/commands/skills/scripts）を ~/.claude に同期する。
# PreToolUse hook は Rust 製バイナリ（modules/home/parts/claude-hooks）で、rebuild でのみ配置される。
# nixos-rebuild を待たずに作業中のスキル変更を即座に反映するためのコマンド。
# home-manager アクティベーション（claude-code.nix）からも同じ実装が呼ばれる。
#
# Usage: claude-sync [source-repo]
#   source-repo 省略時: ~/nix/nixfiles
#
# 同期ポリシー（アクティベーションと同一）:
#   - commands: --ignore-existing でユーザーカスタマイズを保護
#   - CLAUDE.md/skills/scripts: ソースが正として常に上書き。--checksum で中身を比べる
#   - --delete は使わないため、手動追加ファイルは保持される
#   - 外部リポジトリ由来のスキル（claude-code.nix の externalSkills）は flake input
#     から取るため、このコマンドでは同期されない（rebuild 時のアクティベーションのみ）

set -euo pipefail

SOURCE_REPO="${1:-$HOME/nix/nixfiles}"
SOURCE_DIR="$SOURCE_REPO/.claude"
CLAUDE_DIR="$HOME/.claude"

if [ ! -d "$SOURCE_DIR" ]; then
  echo "❌ ソースが見つかりません: $SOURCE_DIR" >&2
  echo "Usage: claude-sync [source-repo]" >&2
  exit 1
fi

mkdir -p "$CLAUDE_DIR"

# 上書きする同期は --checksum で中身を比べる。rsync の既定（サイズと mtime が同じなら飛ばす）では、
# activation の nix store のソース（mtime 1）を -a でコピーした ~/.claude 側も mtime 1 になり、
# 語の置き換えのような同じサイズの変更が飛ばされる。-t を外して時刻を保たない形（--no-times）は、
# mtime が毎回食い違うので全ファイルを毎回書き直す。--checksum は中身の違うファイルだけを書き、
# 両側を読む量は skills と scripts の合わせて 1 MB に満たない。
# 比べた案の記録: https://github.com/tagawa0525/nixfiles/issues/566

# CLAUDE.md: ソースが正として常に上書き（グローバル指針）
if [ -f "$SOURCE_DIR/CLAUDE.md" ]; then
  rsync -a --checksum "$SOURCE_DIR/CLAUDE.md" "$CLAUDE_DIR/CLAUDE.md"
  echo "✅ CLAUDE.md synced"
fi

# commands: ユーザーカスタマイズを保護（既存ファイルは上書きしない）
if [ -d "$SOURCE_DIR/commands" ]; then
  rsync -a --ignore-existing "$SOURCE_DIR/commands/" "$CLAUDE_DIR/commands/"
  echo "✅ commands synced（既存ファイルは保持）"
fi

# skills: ソースが正として常に上書き
if [ -d "$SOURCE_DIR/skills" ]; then
  rsync -a --checksum "$SOURCE_DIR/skills/" "$CLAUDE_DIR/skills/"
  echo "✅ skills synced"
fi

# scripts: ソースが正として常に上書き（スキル横断で使う共有スクリプト）
if [ -d "$SOURCE_DIR/scripts" ]; then
  rsync -a --checksum "$SOURCE_DIR/scripts/" "$CLAUDE_DIR/scripts/"
  echo "✅ scripts synced"
fi

# Nix storeからコピーした読み取り専用ファイルに書き込み権限を付与
chmod -R u+w "$CLAUDE_DIR/CLAUDE.md" "$CLAUDE_DIR/commands" "$CLAUDE_DIR/skills" "$CLAUDE_DIR/scripts" 2>/dev/null || true

echo "🎉 Claude Code 設定を同期しました: $SOURCE_DIR → $CLAUDE_DIR"
