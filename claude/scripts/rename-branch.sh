#!/bin/sh
# rename-branch.sh — 現在の feature ブランチをリネームする。中身は claude-hooks script rename-branch（Rust にして Windows
# でも動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: rename-branch.sh <new-name> [--remote]
#
# - main / master ではリネームしない（exit 1）
# - リモートに同名ブランチがあれば、--remote なしでは何もせず止まる（exit 2）。
#   リモートを書き換える操作なので、ユーザーの承認を得てから --remote を付けて再実行する
# - --remote 指定時: 新名を push（上流を設定）してから旧名をリモートから削除する。
#   ただし旧名を head とする open PR があれば、削除で PR が閉じてしまうため中止する（exit 1）
#
# 出力:
#   RENAMED: <old> -> <new>
#   REMOTE: updated|none

. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script rename-branch "$@"
