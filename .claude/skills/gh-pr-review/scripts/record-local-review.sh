#!/bin/sh
# gh-pr-review: ローカルの /code-review の結果を、PR の head の記録としてコメントに残すスクリプト。中身は
# claude-hooks script record-local-review（Rust にして Windows でも動かす。記録の形は、読むマージの hook の
# pre_merge_check.rs の local_review_mark が決める）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは
# 残して呼ぶ
#
# Usage: record-local-review.sh <PR の URL> <SHA> <high> <medium> <low>
#   SHA は /code-review を回したコミット（git rev-parse HEAD）。PR の head と一致しなければ記録しない
#   high / medium / low は、ローカルレビューで見つけた指摘の数（重大度は Copilot に揃える）
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script record-local-review "$@"
