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
    # 行末のコメントに書いた --no-track で通さないよう、コマンドの部分だけを見る
    if [[ "${line%%#*}" == *--no-track* ]]; then
      _pass
    else
      _fail "上流が origin/... になり、git push の先が main になる"
    fi
  done < <(grep -E 'git (switch|checkout) [^#]*(-c|-C|-b|-B) [^#]* origin/' "$skill" || true)
done

# スキル、スクリプト、CLAUDE.md は PC 全体のセッションで読み込まれる。nixfiles の ADR の番号は、ほかの
# プロジェクトでは参照できないノイズになるので、番号を書かず、その場で規則と理由が分かるように書く。
# ADR-0123、adr-0123、ADR 0123、docs/adr/0123-... のどれも対象にする（書式の例は NNNN と書く）
HOOK_RULES="$CLAUDE_DIR/../modules/home/parts/claude-hooks/src/rules"
ADR_REF='(ADR|adr)[- ]?[0-9]{3,5}|docs/adr/[0-9]{3,5}'

it "skills: ADR の番号の検査の対象（CLAUDE.md、skills、scripts、hook のルール）が見つかる"
if [[ -f "$CLAUDE_DIR/CLAUDE.md" && -d "$CLAUDE_DIR/skills" && -d "$CLAUDE_DIR/scripts" && -d "$HOOK_RULES" ]]; then _pass; else _fail "検査の対象が見つからない（移したなら、このテストの対象も直す）"; fi

while IFS= read -r hit; do
  it "skills: ADR の番号を書かない: $hit"
  _fail "番号を外し、規則と理由をその場で分かるように書く"
done < <(grep -rnE "$ADR_REF" "$CLAUDE_DIR/CLAUDE.md" "$CLAUDE_DIR/skills" "$CLAUDE_DIR/scripts" | cut -c1-200)

# hook の拒否メッセージ（Rust の文字列）も、どのプロジェクトのセッションにも出る。複数行の文字列の
# 継続行も見る。テストのモジュール（#[cfg(test)] 以降）とコメント（行頭と行末の // 以降）は、読み手が nixfiles の開発者なので対象にしない
while IFS= read -r hit; do
  it "skills: hook の拒否メッセージに ADR の番号を書かない: $hit"
  _fail "番号を外し、規則と理由をメッセージの中で分かるように書く"
done < <(perl -ne 'close ARGV if /^#\[cfg\(test\)\]/; next if m{^\s*//}; (my $c = $_) =~ s{\s//.*}{}; print "$ARGV:$.:$c" if $c =~ m{'"$ADR_REF"'}' "$HOOK_RULES"/*.rs | cut -c1-200)

finish
