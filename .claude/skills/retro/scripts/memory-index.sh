#!/usr/bin/env bash
# memory-index.sh — メモリの索引（MEMORY.md）と実ファイル、本文の [[名前]] の参照先を突き合わせる
#
# Usage: memory-index.sh MEMORY_DIR
#   MEMORY_DIR  MEMORY.md のあるディレクトリ（session-events.sh の MEMORY_DIR:）
#
# 出力:
#   ENTRIES: <索引の項目（.md へのリンク）の数>
#   MISSING: <file>              索引にあり、ファイルが無い
#   UNINDEXED: <file>            ファイルがあり、索引に無い（MEMORY.md 自身を除く）
#   BROKEN_LINK: <file> -> <名前>  本文の [[名前]] に、<名前>.md が無い
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

# 索引のリンク先（`](name.md)`）。grep の 0 件は終了コード 1 なので、0 件を空の一覧として扱う
mapfile -t linked < <(grep -oE '\]\([^)]+\.md\)' "$index" | sed -E 's/^\]\((.*)\)$/\1/' || true)
echo "ENTRIES: ${#linked[@]}"

for target in "${linked[@]}"; do
  [[ -e "$dir/$target" ]] || echo "MISSING: $target"
done

shopt -s nullglob
files=("$dir"/*.md)
shopt -u nullglob

for path in "${files[@]}"; do
  file=$(basename "$path")
  [[ "$file" == MEMORY.md ]] && continue
  found=0
  for target in "${linked[@]}"; do
    if [[ "$target" == "$file" ]]; then
      found=1
      break
    fi
  done
  ((found)) || echo "UNINDEXED: $file"
done

# [[名前]] の参照先は、ファイル名の語幹か前付けの name（実際のメモリは両方を使う）
declare -A known=()
for path in "${files[@]}"; do
  file=$(basename "$path")
  known[${file%.md}]=1
  name=$(sed -nE '1,/^---$/{s/^name:[[:space:]]*(.*[^[:space:]])[[:space:]]*$/\1/p}' "$path" | head -n 1)
  [[ -z "$name" ]] || known[$name]=1
done

for path in "${files[@]}"; do
  file=$(basename "$path")
  while IFS= read -r name; do
    [[ -n "${known[$name]:-}" ]] || echo "BROKEN_LINK: $file -> $name"
  done < <(grep -oE '\[\[[^]]+\]\]' "$path" | sed -E 's/^\[\[(.*)\]\]$/\1/' | sort -u || true)
done
