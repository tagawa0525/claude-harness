#!/bin/sh
# memory-index.sh — メモリの索引（MEMORY.md）と実ファイル、本文の [[名前]] の参照先を突き合わせ、本文の崩れた日本語の語を
# 辞書で検査する。中身は claude-hooks script memory-index（Rust にして Windows でも動かす）
#
# Usage: memory-index.sh MEMORY_DIR
#   MEMORY_DIR  MEMORY.md のあるディレクトリ
#
# 出力:
#   ENTRIES: <索引の項目（同じディレクトリの .md へのリンク）の数>
#   MISSING: <file>              索引にあり、ファイルが無い
#   UNINDEXED: <file>            ファイルがあり、索引に無い（MEMORY.md 自身を除く）
#   BROKEN_LINK: <file> -> <名前>  本文の [[名前]]（[[名前|別名]]、[[名前#節]] を含む）に、語幹かフロントマターの
#                                  name が <名前> のファイルが無い。コードの中（`...` と ``` か ~~~ のフェンス）と、名前の空のリンクは読まない
#   TERMS: <file>:<行> <誤り> => <正しい語>
#                                本文（MEMORY.md を含む各 .md）の崩れた日本語の語。commit の pre-commit と同じ辞書と規則
#                                （ja-terms check）で見るので、コードスパン、フェンス、フロントマターの中は数えない
# 食い違いがあっても exit 0（判断はスキルが行う）。MEMORY.md が読めない、または辞書の検査を実行できなければ
# （ほかの行を出した後に）ERROR: を標準エラーに出して exit 1。
#
# 辞書の検査は、hook と同じく claude-hooks の隣の ja-terms（Linux は claude-hooks の package.nix が渡すパス）。メモリは git で
# 追跡しないので pre-commit が掛からず、毎回のセッションに読み込まれて依頼やコミットにコピーされるため、ここで見る
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script memory-index "$@"
