#!/usr/bin/env bash
# ネイティブの Windows の Git Bash で、このスクリプトのある clone から ~/.claude にハーネスを入れる（docs/windows.md）。
# 前もって入れるもの（winget）と開発者モードは docs/windows.md の手順で済ませておく。何度流してもよく、更新は clone を
# pull してからもう一度流す（crate の版は変えないので --force で入れ直す）。
set -euo pipefail

# clone の場所はスクリプトの位置から決める（機械によって ~/nix/claude-harness でも ~/github/claude-harness でもよい）
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"

# claude-hooks を Linux と同じ ~/.claude/bin に置くのは、スキルの sh のスクリプトがこのパスで呼ぶため
for crate in claude-hooks ja-terms commit-refs claude-setup history-rewrite design-records; do
  cargo install --locked --force --root ~/.claude --path "crates/$crate"
done

hook=$(cygpath -w ~/.claude/bin/claude-hooks.exe)
~/.claude/bin/claude-setup sync "$repo"
~/.claude/bin/claude-setup settings --static claude-settings.json --pre-tool-use "$hook" --stop "$hook" --user-prompt-submit "$hook"

# スキルと hook の修正方法の文は claude-setup と commit-refs を名前で呼ぶ。Claude Code の Bash ツールも ~/.bashrc を読む
path_line='export PATH="$HOME/.claude/bin:$PATH"'
if ! grep -qsxF "$path_line" ~/.bashrc; then
  echo "$path_line" >>~/.bashrc
  # 知らせの文の ~ は、利用者に見せるパスの表記で、展開しない
  # shellcheck disable=SC2088
  echo "~/.bashrc で ~/.claude/bin を PATH の先頭に足しました。Git Bash を開き直してください"
fi

# Stop の hook が自動メモリを refs/memory-backup に退避する先。.gitignore は同期したものと自動メモリだけを除外から外す
# （すべてを除外すると、Claude Code の Grep が ~/.claude の中のスキルも自動メモリも見つけなくなる）。上の階層の
# リポジトリの下にあるときは、そのリポジトリとの兼ね合いで決めることなので、作らずに知らせる
top=$(git -C ~/.claude rev-parse --show-toplevel 2>/dev/null) || top=""
if [ -z "$top" ]; then
  git init -q -b main ~/.claude
elif ! [ "$top" -ef ~/.claude ]; then
  # shellcheck disable=SC2088 # 知らせの文の ~ は、利用者に見せるパスの表記で、展開しない
  echo "~/.claude は $top のリポジトリの下にあるので、自動メモリの退避のリポジトリを作りません（docs/windows.md）" >&2
fi
if [ -z "$top" ] || [ "$top" -ef ~/.claude ]; then
  [ -e ~/.claude/.gitignore ] || printf '%s\n' '/*' '!/.gitignore' '!/CLAUDE.md' '!/skills/' '!/scripts/' \
    '!/projects/' '/projects/*/*' '!/projects/*/memory/' >~/.claude/.gitignore
fi

# グローバルの core.hooksPath がほかの置き場を向いていれば、奪わずに止まる。止まってもほかの手順が済んでいるよう最後に置く
~/.claude/bin/claude-setup git-hooks --bin "$hook"
