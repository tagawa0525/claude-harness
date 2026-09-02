#!/usr/bin/env bash
# .claude/skills/*/SKILL.md の動的コマンド（!`...`）のテスト
#
# 実行: bash .claude/tests/skills.sh
#
# Claude Code は `!` 行を実行前に権限チェックへ通し、クォートされていない `{` を
# ブレース展開とみなして拒否する（allowed-tools に載っていても通らない）。
# 例: `git log @{upstream}..HEAD` は "Brace expansion" として deny される。
# `{` を含む引数は必ずクォートの中に置く。

source "$(dirname "$0")/lib.sh"

# unquoted_brace <command>: クォート部分を除いた残りに `{` があれば 0
unquoted_brace() {
  sed -E "s/'[^']*'//g; s/\"[^\"]*\"//g" <<<"$1" | grep -q '{'
}

shopt -s nullglob
skills=("$CLAUDE_DIR"/skills/*/SKILL.md)
shopt -u nullglob

it "skills: SKILL.md が 1 つ以上見つかる（0 件なら探索先が誤り）"
if (( ${#skills[@]} > 0 )); then _pass; else _fail "no SKILL.md under $CLAUDE_DIR/skills"; fi

for skill in "${skills[@]}"; do
  name=$(basename "$(dirname "$skill")")
  while IFS= read -r line; do
    cmd=${line#'!`'}
    cmd=${cmd%'`'}
    it "skills: $name の動的コマンドにクォート外の { がない: $cmd"
    if unquoted_brace "$cmd"; then
      _fail "クォート外の { は権限チェックでブレース展開と判定され deny される"
    else
      _pass
    fi
  done < <(grep -o '!`[^`]*`' "$skill")
done

finish
