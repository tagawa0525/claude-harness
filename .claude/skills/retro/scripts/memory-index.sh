#!/usr/bin/env bash
# memory-index.sh — メモリの索引（MEMORY.md）と実ファイル、本文の [[名前]] の参照先を突き合わせる
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
# 食い違いがあっても exit 0（判断はスキルが行う）。MEMORY.md が読めなければ exit 1。

set -euo pipefail

if (($# != 1)); then
  echo "Usage: memory-index.sh MEMORY_DIR" >&2
  exit 1
fi

dir=$1
index="$dir/MEMORY.md"
if [[ ! -r "$index" ]]; then
  echo "ERROR: 索引を読めません: $index" >&2
  exit 1
fi

# 索引のリンク先（`](name.md)`）。題（`"..."`）、節（`#...`）、先頭の ./ を外し、同じディレクトリの .md だけを
# 残す（外部の URL と別のディレクトリへの参照は索引の項目でない）。
# grep の 0 件は終了コード 1 なので、0 件を空の一覧として扱う
mapfile -t linked < <(
  { grep -oE '\]\([^)]+\)' "$index" || true; } |
    sed -E 's/^\]\((.*)\)$/\1/; s/[[:space:]]+".*"$//; s/#.*$//; s|^\./||' |
    grep -v '/' | grep -E '\.md$' || true
)
echo "ENTRIES: ${#linked[@]}"

declare -A indexed=()
for target in "${linked[@]}"; do
  indexed[$target]=1
  [[ -e "$dir/$target" ]] || echo "MISSING: $target"
done

shopt -s nullglob
files=("$dir"/*.md)
shopt -u nullglob

for path in "${files[@]}"; do
  file=$(basename "$path")
  [[ "$file" == MEMORY.md || -n "${indexed[$file]:-}" ]] || echo "UNINDEXED: $file"
done

# [[名前]] の参照先は、ファイル名の語幹かフロントマターの name（実際のメモリは両方を使う）
declare -A known=()
for path in "${files[@]}"; do
  file=$(basename "$path")
  known[${file%.md}]=1
  # フロントマターは 1 行目の --- から次の --- まで。値の引用符（YAML の "..." と '...'）は外す
  name=$(awk '
    NR == 1 { if ($0 != "---") exit; next }
    $0 == "---" { exit }
    sub(/^name:[[:space:]]*/, "") {
      sub(/[[:space:]]+$/, "")
      if ($0 ~ /^".*"$/ || $0 ~ /^\047.*\047$/) $0 = substr($0, 2, length($0) - 2)
      print
      exit
    }
  ' "$path")
  [[ -z "$name" ]] || known[$name]=1
done

for path in "${files[@]}"; do
  file=$(basename "$path")
  while IFS= read -r name; do
    [[ -z "$name" || -n "${known[$name]:-}" ]] || echo "BROKEN_LINK: $file -> $name"
  done < <(
    # フェンスの中の行と `...` を除いてから [[...]] を拾い、| と # より後を外す
    awk '/^[[:space:]]*(```|~~~)/ { fence = !fence; next } !fence { gsub(/`[^`]*`/, ""); print }' "$path" |
      { grep -oE '\[\[[^]]+\]\]' || true; } |
      sed -E 's/^\[\[(.*)\]\]$/\1/; s/[|#].*$//; s/^[[:space:]]+//; s/[[:space:]]+$//' | sort -u
  )
done
