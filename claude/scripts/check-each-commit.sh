#!/bin/sh
# check-each-commit.sh — <base>..HEAD の各コミットで検査コマンドを実行する。中身は claude-hooks script check-each-commit
# （中断の後片付けを含む流れを、Rust のテストで確かめるため）。スキルの文はこのスクリプトの名前とパスで呼ぶので、
# ファイルは残して呼ぶ
#
# Usage: check-each-commit.sh <base> -- <コマンド…>
#   base      比較元（git の rev）。<base>..HEAD のコミットを古い順に調べる
#   コマンド  各コミットをチェックアウトした一時 worktree の根で実行する。
#             何を通すかは呼び出し側が決める（RED を skip でコミットする運用では
#             「テストが全部通る」は求められず、ビルドだけを通す、など）。
#             コマンドの出力は標準エラーに回し、標準出力は 1 コミット 1 行にする。
#             標準入力は渡さない
#
# 出力（標準出力）: `<短縮hash> <件名> exit=<N>`
# 終了コード: 全コミットで 0 なら 0、1 つでも失敗すれば 1、使い方の誤りは 2、
# 実行時の環境の失敗は 3（git rev-list、一時ディレクトリ、worktree の作成、checkout、clean の失敗。
# shallow clone や壊れたオブジェクトなど）。clean に失敗したコミットは検査しない
# （前のコミットの副産物が残ったまま通ると、誤って exit=0 と出る）。
# 中断（INT、TERM、HUP）されたら、走っている検査のコマンドが終わるのを待って一時 worktree を消し、130 で終わる
# （中断されたコミットの行は出さない）。失敗しても止まらず全コミットを実行する。
#
# 使いどころ: 依存や lock を変えるブランチなど、各コミットでビルドが通るべきとき
# （`git bisect` が使えなくなる）。例:
#   check-each-commit.sh origin/main -- nix build .#default
#
# 現在の worktree と stash には触れず、`git worktree add --detach` の一時 worktree（Unix は ${TMPDIR:-/tmp}、Windows は TMP か TEMP の下）で
# 調べる。一時 worktree は終了時（失敗、中断を含む）に必ず消す（SIGKILL では消せず残る）。
# 呼び出し元の GIT_DIR、GIT_WORK_TREE などは git と検査コマンドに渡さず、一時 worktree の checkout ではフックを止める。
#
# 許可リスト（nix/claude-code.nix）に入れない。`-- <任意のコマンド>` を
# 実行するので、プレフィックス一致で事前承認すると何でも確認なしで走ってしまう。

. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script check-each-commit "$@"
