# shellcheck shell=bash
# 引数のファイルの全体の崩れた日本語の語を検査する（ADR-0014）。git で追跡しない文章（自動メモリ）を、
# pre-commit と同じ辞書と設定で見るための入口（ADR-0074）。JA_TERMS_CONFIG（textlint の設定）と
# JA_TERMS_EXTENSIONS（検査する拡張子。改行区切り）は、textlint.nix の writeShellApplication が与える。
#
# 使い方: ja-terms-files <file>...
# 出力: textlint の unix の形（<file>:<行>:<列>: <誤り> => <正しい語> [Error/prh]）。<file> は引数のとおり
# 終了コード: 0 = 崩れた語が無い、1 = 崩れた語がある、2 = 検査できなかった（読めないファイル、対象でない拡張子、
# textlint の失敗）
#
# ja-terms-message（./message.sh）を使わないのは、件名（最初の行）が Merge や fixup! なら除くなど、コミットの
# メッセージの規則が掛かるため。ファイルの全体を見る pre-commit（./staged.sh）と同じく、Markdown のコード
# スパンとフェンス、フロントマターは textlint のパーサーが外す。

if (($# == 0)); then
  echo "Usage: ja-terms-files <file>..." >&2
  exit 2
fi

mapfile -t extensions <<<"$JA_TERMS_EXTENSIONS"

# 出力のパスを書き出した名前から元に戻すので、textlint が見る実体のパスにする（TMPDIR が symlink を
# 経ると、mktemp の返すパスと textlint が出すパスが違う）
work=$(cd "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$work"' EXIT
mkdir "$work/files"

# textlint は引数をグロブとして読むので、グロブの文字を含まない番号の名前で書き出す。拡張子はプラグインを
# 選ぶのに使うので、小文字にして残す。textlint は知らない拡張子のファイルを検査せずに通すので、対象の拡張子に
# 無いものは検査できないとして止める
: >"$work/names"
n=0
for path in "$@"; do
  base=${path##*/}
  ext=${base##*.}
  ext=${ext,,}
  if [[ "$base" != *.* ]] || ! printf '%s\n' "${extensions[@]}" | grep -qxF -- "$ext"; then
    echo "ERROR: 検査する拡張子ではありません: $path（対象: ${extensions[*]}）" >&2
    exit 2
  fi
  if [[ ! -f "$path" || ! -r "$path" ]]; then
    echo "ERROR: 読めません: $path" >&2
    exit 2
  fi
  name="$n.$ext"
  cp -- "$path" "$work/files/$name"
  printf '%s\t%s\n' "$name" "$path" >>"$work/names"
  n=$((n + 1))
done

rc=0
(cd "$work/files" && textlint --config "$JA_TERMS_CONFIG" --format unix --no-color ./*) >"$work/out" 2>&1 || rc=$?

# 書き出した名前を元のパスに戻して出す（unix の形は「パス:行:列: 中身」）
awk -F'\t' -v dir="$work/files/" '
  NR == FNR { path[$1] = $2; next }
  {
    line = $0
    if (index(line, dir) == 1) line = substr(line, length(dir) + 1)
    split(line, part, ":")
    if (part[1] in path) line = path[part[1]] substr(line, length(part[1]) + 1)
    print line
  }
' "$work/names" "$work/out"

case $rc in
  0 | 1) exit "$rc" ;;
  *) exit 2 ;;
esac
