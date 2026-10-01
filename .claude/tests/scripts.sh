#!/usr/bin/env bash
# .claude/scripts と skills/*/scripts のテストをまとめて実行する
#
# 実行: bash .claude/tests/scripts.sh
# テスト本体は対象ごとに scripts/ に分けてあり、個別にも実行できる。

rc=0
for test in "$(dirname "$0")"/scripts/*.sh; do
  echo "=== $(basename "$test")"
  bash "$test" || rc=1
done
exit "$rc"
