#!/bin/sh
# worktree-add.sh — 命名規則に従って worktree を作る。中身は claude-hooks script worktree-add（Rust にして Windows でも
# 動かす。.worktreelink の規則の詳細もそこに書く）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して
# 呼ぶ
#
# Usage: worktree-add.sh <branch> [--from <ref>] [--carry-changes]
#
# 作成先はメイン worktree の親ディレクトリの `<repo>-<branch>`（ブランチ名の / は - に変換）。ブランチの状態は自動判定する:
#   - ローカルにある       → そのままチェックアウト
#   - リモートにだけある   → 追跡ブランチを作ってチェックアウト
#   - どこにもない         → 既定ブランチ（origin/HEAD → origin/main → origin/master → main → master の最初にあるもの）
#                            から上流なしで新規作成。今の HEAD から作ると、作業中のブランチの未完了のコミットが別の
#                            トピックに入るため
# --from <ref>: 新規作成の起点を明示する（今のブランチの続きなら --from HEAD）。既存のブランチに付けたらエラー。
# --carry-changes: 未コミットの変更（未追跡ファイル含む）を stash 経由で worktree 側に移す
#   （git-commit スキルの「main からの退避」）。
#
# 共有（.worktreelink）: 新しい worktree のルートに .worktreelink があれば、1 行 1 パス（ルートからの相対パス）を、
# メイン worktree の同じパスへの絶対パスの symlink として作る。git で追跡しない大きなデータを worktree の間で共有するため。
# パスはリテラルで、新しい worktree で git に無視され、メイン worktree で追跡されていないものだけ（dir/ でなく dir か
# /dir と書く）。Windows の symlink は開発者モードか管理者の権限が要る。
#
# GitHub リモートがあり、ローカルにブランチが無く --from も無ければ、先に origin を fetch する（失敗したら止める）。
#
# 出力: WORKTREE、BRANCH、CREATED_BRANCH（yes|no）、BASE（新規作成のときの起点）、SHARED（共有したパスごと）、NOTE
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script worktree-add "$@"
