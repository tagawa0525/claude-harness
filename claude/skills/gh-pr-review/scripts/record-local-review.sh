#!/bin/sh
# gh-pr-review: ローカルの /code-review の結果を、PR の head の記録としてコメントに残すスクリプト。中身は
# claude-hooks script record-local-review（Rust にして Windows でも動かす。記録の形は、読むマージの hook の
# pre_merge_check.rs の local_review_mark が決める）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは
# 残して呼ぶ
#
# Usage: record-local-review.sh <PR の URL> <SHA> <high> <medium> <low> [--unlogged <理由>]
#   SHA は /code-review を回したコミット（git rev-parse HEAD）。PR の head と一致しなければ記録しない
#   PR の worktree で実行する（その head で /code-review を呼んだ行を、起動したディレクトリの clone で確かめる。無ければ
#   記録せず、次の手を出す。hook が行を書かない回し方をしたときだけ --unlogged で理由を残す）
#   high / medium / low は、ローカルレビューで見つけた指摘の数（重大度は Copilot に揃える）
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script record-local-review "$@"
