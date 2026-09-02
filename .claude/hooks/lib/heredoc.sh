#!/usr/bin/env bash
# hooks 共通: コマンド文字列のヒアドキュメント本文を空白で覆う（マスクする）
#
# hook は Bash ツールのコマンド文字列を正規表現で検査するが、ヒアドキュメントの
# 本文は「これから実行されるコマンド」ではなくデータ（ファイルの中身、PR 本文、
# コミットメッセージ）である。本文に現れるだけのコマンド例に反応すると、
# 説明文を書くだけの操作が deny され、迂回のために文言を変える羽目になる。
#
# 本文を削除せず同じ長さの空白に置き換えるのは、**元の文字列との位置が一致する**
# ようにするため。検出した位置をそのまま元の文字列に適用でき、
# 「検出はマスク後・本文の中身は元の文字列」を取り違えずに切り出せる。
#
# 使い方:
#   source "$(dirname "${BASH_SOURCE[0]}")/lib/heredoc.sh"
#   COMMAND_RAW="$COMMAND"
#   COMMAND=$(mask_heredoc_bodies <<<"$COMMAND")     # 検出・フラグ解析はこちら
#   # 検出後、本文の中身を読む必要があるとき:
#   PREFIX="${COMMAND%%"${BASH_REMATCH[0]}"*}"
#   OFFSET=$(( ${#PREFIX} + ${#BASH_REMATCH[0]} ))
#   PART="${COMMAND:OFFSET}"          # フラグ解析用（本文はマスク済み）
#   PART_RAW="${COMMAND_RAW:OFFSET}"  # 本文の中身を読む用
#
# 長さの数え方を bash の ${#var} に統一するため、実装は awk ではなく bash で行う
# （awk の実装によっては多バイト文字を文字ではなくバイトで数え、位置がずれる）。
#
# 既知の限界: クォートの中に現れる `<<`（例: echo "a << b"）を開始と誤認しうる。
# その場合は以降の行が本文としてマスクされ、hook の検査対象が狭まる（deny の
# 取りこぼし方向）。誤検出で操作が止まるより無害なため、この単純さを選ぶ。

# mask_heredoc_bodies: 標準入力のコマンド文字列を読み、ヒアドキュメントの本文行と
# 終端行を同じ長さの空白に置き換えて標準出力に返す。開始行はそのまま残す。
mask_heredoc_bodies() {
  local -a lines=() out=() delims=() dashes=()
  local line scan body d
  mapfile -t lines

  for line in "${lines[@]}"; do
    if (( ${#delims[@]} > 0 )); then
      body="$line"
      # <<- はタブでインデントした終端子を許す
      (( dashes[0] )) && body="${body#"${body%%[!$'\t']*}"}"
      if [[ "$body" == "${delims[0]}" ]]; then
        delims=("${delims[@]:1}")
        dashes=("${dashes[@]:1}")
      fi
      # 元の行と同じ文字数の空白に置き換える（位置を保つ）
      out+=("$(printf '%*s' "${#line}" '')")
      continue
    fi

    out+=("$line")

    # この行で始まるヒアドキュメントの終端子を出現順に集める。
    # <<< はヒアストリング（本文を持たない）なので、走査用のコピーから潰しておく
    scan="${line//<<</%%%}"
    while [[ "$scan" =~ (\<\<-?)[[:space:]]*(\"[^\"]*\"|\'[^\']*\'|[A-Za-z_][A-Za-z0-9_]*) ]]; do
      d="${BASH_REMATCH[2]}"
      d="${d%\"}"; d="${d#\"}"
      d="${d%\'}"; d="${d#\'}"
      delims+=("$d")
      if [[ "${BASH_REMATCH[1]}" == "<<-" ]]; then dashes+=(1); else dashes+=(0); fi
      scan="${scan#*"${BASH_REMATCH[0]}"}"
    done
  done

  (( ${#out[@]} > 0 )) && printf '%s\n' "${out[@]}"
}
