# shellcheck shell=sh
# claude-hooks-script.sh — 中身を claude-hooks script に移したスクリプト（ADR-0087）の sh が、claude-hooks を起動する
# （source して使う）
#
# claude_hooks_script <名前> [引数]...
#   claude-hooks script <名前> [引数]... を exec する。バイナリは CLAUDE_HOOKS_BIN（テストがブランチのバイナリに
#   差し替える）か ~/.claude/bin/claude-hooks で、PATH では引かない（lib/local-review.sh と同じ）。
#   バイナリが無ければ、入れ方を示して 127 で終える。sh の `exec: not found` だけでは何を入れるかが分からず、claude-sync は
#   スクリプトだけを入れてバイナリを入れない。バイナリが古く <名前> を知らなければ、claude-hooks 自身が入れ直しの手順を示す

claude_hooks_script() {
  claude_hooks_bin="${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}"
  # Windows の Git Bash は .exe を補って実行するので、.exe の付いたものも在るものとして数える
  if [ ! -e "$claude_hooks_bin" ] && [ ! -e "$claude_hooks_bin.exe" ]; then
    echo "ERROR: ${claude_hooks_bin} がありません。Linux は home-manager の rebuild、Windows は claude-harness の README の「Windows で使う」の cargo install で入れてください（claude-sync はバイナリを入れない）" >&2
    exit 127
  fi
  exec "$claude_hooks_bin" script "$@"
}
