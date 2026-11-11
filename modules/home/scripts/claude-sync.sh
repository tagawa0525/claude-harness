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
#   source-repo 省略時: ~/nix/claude-harness（このスクリプトと .claude を持つリポジトリの clone）。同期の前に
#     git pull --ff-only で最新にする。既定のブランチでない、同期する場所に未コミットの変更がある、pull できない、の
#     どれかなら、同期せずに止まる
#   source-repo を渡したとき: pull せず、その場所の中身をそのまま同期する（worktree で直しを試す、rebuild の
#     activation、pull できないときに今の中身を入れる）
#
# 同期ポリシー（アクティベーションと同一）:
#   - commands: --ignore-existing でユーザーカスタマイズを保護
#   - CLAUDE.md/skills/scripts: ソースが正として常に上書き。--checksum で中身を比べる
#   - --delete は使わないため、手動追加ファイルは保持される
#   - 外部リポジトリ由来のスキル（claude-code.nix の externalSkills）は flake input
#     から取るため、このコマンドでは同期されない（rebuild 時のアクティベーションのみ）

set -euo pipefail

DEFAULT_REPO="$HOME/nix/claude-harness"
SOURCE_REPO="${1:-$DEFAULT_REPO}"
SOURCE_DIR="$SOURCE_REPO/.claude"
CLAUDE_DIR="$HOME/.claude"

# 引数なしは「ハーネスの最新を入れる」ための呼び出しなので、入れるのは既定の clone の、既定のブランチの、コミットされた
# 最新だけにする。clone が古いまま、別のブランチのまま、書きかけの編集を持ったまま同期すると、rebuild で配られた新しい版を、
# 黙ってそれで上書きする。作業中の中身を入れたいときは、パスを渡す（下の案内）。
# 手元の既定のブランチが origin より先（まだ push していないコミット）のときは、その中身を入れる（古くはない）。
# 既定の場所が clone でなければ（.git が無い。親のリポジトリの中のコピーなど）、pull するものが無いのでそのまま進む
if [ -z "${1:-}" ] && [ -e "$SOURCE_REPO/.git" ]; then
  # GIT_DIR などを export したシェル（git の hook の中など）から呼ばれても、既定の clone を相手にする
  while IFS= read -r var; do unset "$var"; done < <(git rev-parse --local-env-vars)
  as_is="今の中身をそのまま同期するなら: claude-sync $SOURCE_REPO"
  # git を実行できない、clone として読めない（safe.directory の拒否、壊れた .git）ときは、下の判定が既定値に落ちて
  # 誤った理由を示すので、先に止める
  if ! git -C "$SOURCE_REPO" rev-parse --git-dir >/dev/null; then
    echo "❌ $SOURCE_REPO を git のリポジトリとして読めません（上の出力を参照）。古い中身を同期しないよう、ここで止めます" >&2
    echo "   $as_is" >&2
    exit 1
  fi
  default=$(git -C "$SOURCE_REPO" symbolic-ref --short -q refs/remotes/origin/HEAD || true)
  default=${default#origin/}
  default=${default:-main}
  branch=$(git -C "$SOURCE_REPO" symbolic-ref --short -q HEAD || echo "detached HEAD")
  if [ "$branch" != "$default" ]; then
    echo "❌ $SOURCE_REPO は $default でなく $branch にあります。古い中身を同期しないよう、ここで止めます" >&2
    echo "   最新を入れるなら: git -C $SOURCE_REPO switch $default のあと、もう一度 claude-sync" >&2
    echo "   $as_is" >&2
    exit 1
  fi
  # 見るのは同期する場所だけ（下の rsync の 4 つ）。追跡していない新しいファイルも、同期されるので数える
  if ! dirty=$(git -C "$SOURCE_REPO" status --porcelain --untracked-files=all -- \
    .claude/CLAUDE.md .claude/commands .claude/skills .claude/scripts); then
    echo "❌ $SOURCE_REPO の未コミットの変更を調べられません（上の出力を参照）。ここで止めます" >&2
    echo "   $as_is" >&2
    exit 1
  fi
  if [ -n "$dirty" ]; then
    echo "❌ $SOURCE_REPO の同期する場所に未コミットの変更があります。書きかけの中身を同期しないよう、ここで止めます" >&2
    echo "   $as_is" >&2
    exit 1
  fi
  if ! git -C "$SOURCE_REPO" pull -q --ff-only; then
    echo "❌ $SOURCE_REPO を git pull --ff-only で最新にできません（上の git の出力を参照）。古い中身を同期しないよう、ここで止めます" >&2
    echo "   $as_is" >&2
    exit 1
  fi
  echo "📥 $SOURCE_REPO: $default $(git -C "$SOURCE_REPO" rev-parse --short HEAD)"
fi

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
