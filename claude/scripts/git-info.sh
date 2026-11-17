#!/bin/sh
# git-info.sh — 現在の Git 状態を俯瞰する（git-info スキルの収集・整形部分）。中身は claude-hooks script git-info（Rust
# にして Windows でも動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: git-info.sh
#
# ブランチ・未コミット変更・未プッシュコミット・stash・worktree・関連 PR を表示し、
# 既定ブランチ（main / master）にマージ済みのブランチを持つ worktree を検出する。
# 情報収集なので個別コマンドの失敗では止めず、代替表示にする。
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script git-info "$@"
