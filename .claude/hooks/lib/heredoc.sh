#!/usr/bin/env bash
# hooks 共通: コマンド文字列からヒアドキュメント本文を取り除く
#
# hook は Bash ツールのコマンド文字列を正規表現で検査するが、ヒアドキュメントの
# 本文は「これから実行されるコマンド」ではなくデータ（ファイルの中身、PR 本文、
# コミットメッセージ）である。本文に現れるだけのコマンド例に反応すると、
# 説明文を書くだけの操作が deny され、迂回のために文言を変える羽目になる。
#
# 使い方（hook の先頭で source し、検出とフラグ解析には戻り値を使う）:
#   source "$(dirname "${BASH_SOURCE[0]}")/lib/heredoc.sh"
#   COMMAND_EXEC=$(strip_heredoc_bodies <<<"$COMMAND")
#
# 本文の中身そのものを読む検査（PR 本文の見出しなど）には元の文字列を使う。
# 本文はヒアドキュメントで渡されるのが普通なので、そこを削ると読めなくなる。
#
# 既知の限界: クォートの中に現れる `<<` （例: echo "a << b"）を開始と誤認しうる。
# その場合は以降の行が本文として捨てられ、hook の検査対象が狭まる（deny の
# 取りこぼし方向）。誤検出で操作が止まるより無害なため、この単純さを選ぶ。

# strip_heredoc_bodies: 標準入力のコマンド文字列から、ヒアドキュメントの本文と
# 終端行を取り除いて標準出力に返す。開始行（`cat <<'EOF'` の行）は残す。
strip_heredoc_bodies() {
  awk '
    # 未終端のヒアドキュメント終端子をキューで持つ（1 行に複数開始できるため）
    BEGIN { n = 0 }
    {
      if (n > 0) {
        line = $0
        # <<- はタブでインデントされた終端子を許す
        if (dash[1]) { sub(/^\t+/, "", line) }
        if (line == delim[1]) {
          for (i = 1; i < n; i++) { delim[i] = delim[i + 1]; dash[i] = dash[i + 1] }
          n--
        }
        next
      }

      print

      rest = $0
      while (match(rest, /<<-?[ \t]*("[^"]*"|'"'"'[^'"'"']*'"'"'|[A-Za-z_][A-Za-z0-9_]*)/)) {
        tok = substr(rest, RSTART, RLENGTH)
        # <<< はヒアストリング（本文を持たない）
        if (substr(rest, RSTART, 3) == "<<<") {
          rest = substr(rest, RSTART + 3)
          continue
        }
        rest = substr(rest, RSTART + RLENGTH)

        isdash = (tok ~ /^<<-/) ? 1 : 0
        d = tok
        sub(/^<<-?[ \t]*/, "", d)
        gsub(/^["'"'"']|["'"'"']$/, "", d)

        n++
        delim[n] = d
        dash[n] = isdash
      }
    }
  '
}
