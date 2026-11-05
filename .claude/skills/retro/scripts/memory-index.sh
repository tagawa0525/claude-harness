#!/usr/bin/env bash
# memory-index.sh — メモリの索引（MEMORY.md）と実ファイル、本文の [[名前]] の参照先を突き合わせ、本文の崩れた日本語の語を
# 辞書で検査する
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
#                                （ja-terms-files）で見るので、コードスパン、フェンス、フロントマターの中は数えない
# 食い違いがあっても exit 0（判断はスキルが行う）。MEMORY.md が読めない、または辞書の検査を実行できなければ
# （ほかの行を出した後に）ERROR: を標準エラーに出して exit 1。
#
# 辞書の検査は ~/.claude/bin/ja-terms-files（claude-code.nix が置く。JA_TERMS_FILES で差し替える）。メモリは git で
# 追跡しないので pre-commit が掛からず、毎回のセッションに読み込まれて依頼やコミットにコピーされるため、ここで見る

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

# 本文の崩れた語。ja-terms-files は引数のパスのまま「<path>:<行>:<列>: <中身> [Error/prh]」を出すので、ディレクトリを
# 外して列と印を落とす。パスは既知のファイル名との完全な前方一致で切る（名前の : で切り違えない）。files には
# 少なくとも MEMORY.md が入る
ja_terms_files="${JA_TERMS_FILES:-$HOME/.claude/bin/ja-terms-files}"
rc=0
report=$("$ja_terms_files" "${files[@]}") || rc=$?
case "$rc" in
  0 | 1) ;;
  127)
    echo "ERROR: 辞書の検査（${ja_terms_files}）がありません。home-manager の rebuild で入れてください（claude-sync はバイナリを入れない）" >&2
    exit 1
    ;;
  *)
    printf '%s\n' "$report" >&2
    echo "ERROR: 辞書の検査（${ja_terms_files}）が失敗しました（終了コード ${rc}）" >&2
    exit 1
    ;;
esac
terms=$(
  printf '%s\n' "$report" | DIR="$dir/" awk '
    NR == FNR { name[$0] = 1; next }
    index($0, ENVIRON["DIR"]) == 1 {
      rest = substr($0, length(ENVIRON["DIR"]) + 1)
      for (f in name) {
        if (index(rest, f ":") != 1) continue
        body = substr(rest, length(f) + 2)
        if (!match(body, /^[0-9]+:[0-9]+: /)) continue
        split(body, pos, ":")
        msg = substr(body, RLENGTH + 1)
        sub(/ \[[^]]*\]$/, "", msg)
        print "TERMS: " f ":" pos[1] " " msg
      }
    }
  ' <(for path in "${files[@]}"; do basename "$path"; done) -
)
# 崩れた語があると言いながら 1 行も読めなければ、出力の形が変わっている。黙って 0 件にしない
if ((rc == 1)) && [[ -z "$terms" ]]; then
  printf '%s\n' "$report" >&2
  echo "ERROR: 辞書の検査（${ja_terms_files}）の出力を読めません（崩れた語があるが、<path>:<行>:<列>: の行が無い）" >&2
  exit 1
fi
[[ -z "$terms" ]] || printf '%s\n' "$terms"
