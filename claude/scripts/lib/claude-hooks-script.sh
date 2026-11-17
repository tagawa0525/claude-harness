# shellcheck shell=sh
# claude-hooks-script.sh — 中身を claude-hooks script に移したスクリプトの sh が、claude-hooks を起動する（source して
# 使う）。スキルの文と PreToolUse のルールはスクリプトの名前とパスで呼び、見分けるので、sh のファイルは残してこれを呼ぶ
#
# claude_hooks_script <名前> [引数]...
#   claude-hooks script <名前> [引数]... を exec する。バイナリは CLAUDE_HOOKS_BIN（テストがブランチのバイナリに
#   差し替える）か ~/.claude/bin/claude-hooks で、PATH には ~/.claude/bin が無いことがあるので、PATH では引かない。
#   バイナリが無ければ、入れ方を示して 127 で終える。sh の `exec: not found` だけでは何を入れるかが分からず、claude-sync は
#   スクリプトだけを入れてバイナリを入れない。バイナリが古く <名前> を知らなければ、claude-hooks 自身が入れ直しの手順を示す

claude_hooks_script() {
  claude_hooks_bin="${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}"
  # Windows の claude-hooks は .exe の付いた名前で入る。.exe だけがあれば、それを起動する（.exe を補って起動しない sh
  # でも動くように）
  if [ ! -e "$claude_hooks_bin" ] && [ -e "$claude_hooks_bin.exe" ]; then
    claude_hooks_bin="$claude_hooks_bin.exe"
  fi
  if [ ! -e "$claude_hooks_bin" ]; then
    echo "ERROR: ${claude_hooks_bin} がありません。Linux は home-manager の rebuild、Windows は claude-harness の README の「Windows で使う」の cargo install で入れてください（claude-sync はバイナリを入れない）" >&2
    exit 127
  fi
  # 案内に書くスクリプトのパスを、呼んだ sh と同じツリーから引くため（デプロイ先の古い版を案内しない）、sh の場所を
  # 渡す。Windows の Git Bash の pwd は /c/... の形でネイティブの exe が読めないので、-W の C:/... の形にする
  CLAUDE_HOOKS_SCRIPT_DIR=$(cd "$(dirname "$0")" && { pwd -W 2>/dev/null || pwd; })
  export CLAUDE_HOOKS_SCRIPT_DIR
  exec "$claude_hooks_bin" script "$@"
}
