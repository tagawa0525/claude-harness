#!/bin/sh
# rename-plan.sh — docs/plans/ のランダム名の計画書を連番つきの名前にする。中身は claude-hooks script rename-plan（Rust
# にして Windows でも動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: rename-plan.sh --list
#        rename-plan.sh <branch-name> [--file <path>]
#
# Claude Code の plan mode は plansDirectory（docs/plans）にランダム名で計画書を置く。
# ブランチ名が決まったら `NNN_<name>.md` にリネームして追跡しやすくする。
#
# --list: 連番プレフィックス（NNN_）のない .md を新しい順に出す（ブランチ名を決める材料）
# リネーム:
#   - 対象は --file、省略時はランダム名の計画書がちょうど 1 つのときそれ。複数なら exit 1
#   - NNN は既存の最大番号 + 1（なければ 001）
#   - name はブランチ名から先頭の type/ を除き、- と / を _ に変換したもの
#   - git 追跡下なら git mv してコミット。未追跡（gitignore 等）なら mv のみ
#
# 出力:
#   RENAMED: <old> -> <new>
#   COMMITTED: yes|no

. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script rename-plan "$@"
