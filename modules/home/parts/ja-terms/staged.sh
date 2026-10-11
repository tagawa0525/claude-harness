# ステージしたファイルの崩れた日本語の語を検査する（ADR-0014）。グローバルの pre-commit が、リポジトリの
# 直下で呼ぶ。JA_TERMS_CONFIG（textlint の設定）と JA_TERMS_PATHSPECS（検査するファイルの pathspec。改行
# 区切り）は、git.nix の writeShellApplication が与える。
#
# 終了コード: 0 = 崩れた語が無い、1 = 崩れた語がある、2 = textlint が失敗した

# 出力のパスを書き出した名前から元に戻すので、textlint が見る実体のパスにする（TMPDIR がシンボリックリンクを
# 経ると、mktemp の返すパスと textlint が出すパスが違う）
work=$(cd "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$work"' EXIT
mkdir "$work/files"

mapfile -t pathspecs <<<"$JA_TERMS_PATHSPECS"

# ステージしたファイルの、モードとブロブとパス。シンボリックリンクの行き先とサブモジュール（gitlink）は
# 文章ではないので外す
# （--raw -z は「:旧モード 新モード 旧ブロブ 新ブロブ 状態」と「パス」を NUL で区切って並べる。名前の変更は
# パスを 2 つ並べるので、--no-renames で追加と削除に分けて 1 つにする）
# git の出力は一度ファイルに受ける（プロセス置換の中の失敗は set -e に見えず、何も検査せずに通るため）
git diff --cached --raw -z --no-abbrev --no-renames --diff-filter=ACMR -- "${pathspecs[@]}" >"$work/staged"
declare -A blob=()
paths=()
while IFS= read -r -d '' meta && IFS= read -r -d '' path; do
  read -r _ mode _ sha _ <<<"$meta"
  case $mode in 120000 | 160000) continue ;; esac
  blob[$path]=$sha
  paths+=("$path")
done <"$work/staged"
[ "${#paths[@]}" -gt 0 ] || exit 0

# リポジトリの .textlintignore（ステージした版）に挙げたファイルを外す。書き出す名前は元の名前と違うので
# textlint には任せず、同じ gitignore の書き方を git 自身に当てさせる
if git cat-file -e :.textlintignore 2>/dev/null; then
  git show :.textlintignore >"$work/ignore"
  git ls-files -z --cached --ignored --exclude-from="$work/ignore" -- "${paths[@]}" >"$work/ignored"
  while IFS= read -r -d '' path; do
    # 単引用符で、名前を 2 回展開させない（] や $(...) を含む名前）
    unset -v 'blob[$path]'
  done <"$work/ignored"
fi

# textlint は引数をグロブとして読むので、グロブの文字を含まない番号の名前で書き出す。拡張子はプラグインを
# 選ぶのに使うので、小文字にして残す
: >"$work/names"
n=0
for path in "${paths[@]}"; do
  [ -n "${blob[$path]+set}" ] || continue
  ext=${path##*.}
  name="$n.${ext,,}"
  git cat-file blob "${blob[$path]}" >"$work/files/$name"
  printf '%s\t%s\n' "$name" "$path" >>"$work/names"
  n=$((n + 1))
done
[ "$n" -gt 0 ] || exit 0

echo "🔍 Checking Japanese terms..."
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
  0) exit 0 ;;
  1)
    echo "❌ 崩れた日本語の語があります。上の「誤り => 正しい語」に直してください（辞書: nixfiles の modules/home/parts/ja-terms/prh.yml）"
    exit 1
    ;;
  *)
    echo "❌ textlint が失敗しました（終了コード $rc）。上の出力を見てください"
    exit 2
    ;;
esac
