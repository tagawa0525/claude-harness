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

# リモート追跡ブランチ（origin/main など）から作るブランチは、それを上流にすると、引数なしの git push が
# main への push とみなされ guard-git-push に止められる。--no-track で作り、push.autoSetupRemote に
# 同名のリモートブランチを作らせる（worktree-add.sh と同じ）
for skill in "${skills[@]}"; do
  name=$(basename "$(dirname "$skill")")
  while IFS= read -r line; do
    it "skills: $name のリモート追跡ブランチから作るブランチは --no-track: $line"
    if [[ "$line" == *--no-track* ]]; then
      _pass
    else
      _fail "上流が origin/... になり、git push の先が main になる"
    fi
  done < <(grep -E 'git (switch|checkout) [^#]*(-c|-C|-b|-B) [^#]* origin/' "$skill" || true)
done

finish
