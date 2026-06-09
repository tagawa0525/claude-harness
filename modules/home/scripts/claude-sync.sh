#!/usr/bin/env bash
# =============================================================================
# Claude Code Config Sync
# =============================================================================
# リポジトリの .claude（commands/skills/hooks）を ~/.claude に同期する。
# nixos-rebuild を待たずに作業中のスキル変更を即座に反映するためのコマンド。
# home-manager アクティベーション（claude-code.nix）からも同じ実装が呼ばれる。
#
# Usage: claude-sync [source-repo]
#   source-repo 省略時: ~/nix/nixfiles
#
# 同期ポリシー（アクティベーションと同一）:
#   - commands: --ignore-existing でユーザーカスタマイズを保護
#   - skills/hooks: ソースが正として常に上書き
#   - --delete は使わないため、手動追加ファイルは保持される

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

# commands: ユーザーカスタマイズを保護（既存ファイルは上書きしない）
if [ -d "$SOURCE_DIR/commands" ]; then
  rsync -a --ignore-existing "$SOURCE_DIR/commands/" "$CLAUDE_DIR/commands/"
  echo "✅ commands synced（既存ファイルは保持）"
fi

# skills: ソースが正として常に上書き
if [ -d "$SOURCE_DIR/skills" ]; then
  rsync -a "$SOURCE_DIR/skills/" "$CLAUDE_DIR/skills/"
  echo "✅ skills synced"
fi

# hooks: ソースが正として常に上書き
if [ -d "$SOURCE_DIR/hooks" ]; then
  rsync -a "$SOURCE_DIR/hooks/" "$CLAUDE_DIR/hooks/"
  echo "✅ hooks synced"
fi

# Nix storeからコピーした読み取り専用ファイルに書き込み権限を付与
chmod -R u+w "$CLAUDE_DIR/commands" "$CLAUDE_DIR/skills" "$CLAUDE_DIR/hooks" 2>/dev/null || true

echo "🎉 Claude Code 設定を同期しました: $SOURCE_DIR → $CLAUDE_DIR"
