#!/usr/bin/env bash
# ネイティブの Windows の Git Bash で、このスクリプトのある clone から ~/.claude にハーネスを入れる（docs/windows.md）。
# 前もって入れるもの（winget）と開発者モードは docs/windows.md の手順で済ませておく。何度流してもよく、更新は clone を
# pull してからもう一度流す（crate の版は変えないので --force で入れ直す）。
set -euo pipefail

# clone の場所はスクリプトの位置から決める（機械によって ~/nix/claude-harness でも ~/github/claude-harness でもよい）
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"

crates=(claude-hooks ja-terms commit-refs claude-setup history-rewrite design-records)

# Windows は動いている exe の上へ置き換えるのを拒む（os error 5）が、リネームと、Git Bash の rm での削除は許す（動いている
# プロセスはそのまま動く）。ほかのセッションの hook が動かしている間も入れ直せるよう、入っている exe を退けてから入れ、
# 入れた後に消す。settings.json が指す exe が無くなると、すべてのセッションの hook が動かなくなるので、入れるのに
# 失敗したときも、途中で止められたとき（Git Bash は SIGINT、SIGTERM、SIGHUP でも EXIT の trap を実行する）も、退けた exe を戻す
pending=""
build=""
restore() {
  if [ -n "$pending" ] && ! [ -e "${pending%.old-*}" ]; then
    mv "$pending" "${pending%.old-*}" || echo "退けた exe を戻せませんでした。$pending を ${pending%.old-*} に戻してください" >&2
  fi
}
trap 'restore; [ -z "$build" ] || rm -rf "$build"' EXIT
# 消せなくても入れ直しは済んでいるので、止めずに知らせる（次の回にまた消す）
remove_old() {
  rm -f "$1" || echo "退けた exe を消せませんでした（次の回にまた消します）: $1" >&2
}

# 前の回が戻す前に止まって残した退けた exe は、対の exe が無ければそれが唯一の版なので戻し、あれば消す。
# 同じ exe の残りが複数あれば、新しいもの（名前の時刻が大きく、glob の順で後のもの）を戻す
olds=(~/.claude/bin/*.exe.old-*)
for ((i = ${#olds[@]} - 1; i >= 0; i--)); do
  old=${olds[i]}
  if ! [ -e "$old" ]; then
    continue
  elif [ -e "${old%.old-*}" ]; then
    remove_old "$old"
  else
    mv "$old" "${old%.old-*}"
  fi
done

# 入っている exe に触れる前に、一時の置き場へ入れてビルドを済ませる（失敗しても、入っている版はそのまま動く）。
# --path の install は crate の target（crates/<crate>/target）でビルドするので、下の ~/.claude への install は
# ビルド済みのものを置くだけで終わり（手元の測定で 0.07 秒）、exe の無い間は短い。target は回をまたいで残る
build=$(mktemp -d)
for crate in "${crates[@]}"; do
  cargo install --locked --force --root "$build" --path "crates/$crate"
done

# claude-hooks を Linux と同じ ~/.claude/bin に置くのは、スキルの sh のスクリプトがこのパスで呼ぶため
for crate in "${crates[@]}"; do
  exe=~/.claude/bin/$crate.exe
  if [ -e "$exe" ]; then
    pending="$exe.old-$(date +%s)-$$"
    mv "$exe" "$pending"
  fi
  cargo install --locked --force --root ~/.claude --path "crates/$crate"
  if [ -n "$pending" ]; then
    old=$pending
    pending=""
    remove_old "$old"
  fi
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

# 利用者の attributes と core.hooksPath を奪わずに止まりうる 2 つは、止まってもほかの手順が済んでいるよう最後に置き、
# 片方が止まっても、もう片方は流す
rc=0
~/.claude/bin/claude-setup git-attributes --from config/git/attributes || rc=1
~/.claude/bin/claude-setup git-hooks --bin "$hook" || rc=1
exit "$rc"
