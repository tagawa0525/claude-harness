# shellcheck shell=bash
# ステージした版（コミットに入る中身）の Nix、Python、Markdown、Rust の書式と lint を検査する。グローバルの
# pre-commit が、リポジトリの直下で呼ぶ。道具（nixfmt、ruff、markdownlint、rustfmt と cargo）が PATH に
# 無ければ、その検査を飛ばす。
#
# 作業ツリーは書き換えない。一部だけステージしたファイルで、ステージしていない編集を検査せず、コミットに
# 入れないため（ADR-0028）。Markdown の自動修正はステージした版に当ててステージし直し、作業ツリーの
# ファイルがステージした版と同じときだけ、作業ツリーにも書く。
#
# 終了コード: 0 = 違反が無い、1 = 違反がある

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failed=0

# collect <pathspec>...: ステージした普通のファイルのパスとブロブを PATHS と BLOBS に並べる。シンボリック
# リンクの行き先とサブモジュール（gitlink）は外す（--raw -z の読み方は ja-terms/staged.sh と同じ）。
# git の出力は一度ファイルに受ける（プロセス置換の中の失敗は set -e に見えない）
collect() {
  git diff --cached --raw -z --no-abbrev --no-renames --diff-filter=ACMRT -- "$@" >"$work/raw"
  PATHS=()
  BLOBS=()
  local meta path mode sha
  while IFS= read -r -d '' meta && IFS= read -r -d '' path; do
    read -r _ mode _ sha _ <<<"$meta"
    case $mode in 120000 | 160000) continue ;; esac
    PATHS+=("$path")
    BLOBS+=("$sha")
  done <"$work/raw"
}

# --- Nix --------------------------------------------------------------------------------------------
collect '*.nix'
if [ "${#PATHS[@]}" -gt 0 ] && command -v nixfmt >/dev/null 2>&1; then
  echo "🔍 Checking Nix format..."
  bad=()
  for i in "${!PATHS[@]}"; do
    git cat-file blob "${BLOBS[$i]}" | nixfmt --check - >/dev/null 2>&1 || bad+=("${PATHS[$i]}")
  done
  if [ "${#bad[@]}" -gt 0 ]; then
    echo "❌ Nix format check failed: ${bad[*]}. Run: nixfmt <files>"
    failed=1
  fi
fi

# --- Python -----------------------------------------------------------------------------------------
# --stdin-filename の場所から、ruff はプロジェクトの設定を探す
collect '*.py'
if [ "${#PATHS[@]}" -gt 0 ] && command -v ruff >/dev/null 2>&1; then
  echo "🔍 Checking Python format..."
  bad=()
  for i in "${!PATHS[@]}"; do
    git cat-file blob "${BLOBS[$i]}" | ruff format --check --stdin-filename "${PATHS[$i]}" - >/dev/null 2>&1 ||
      bad+=("${PATHS[$i]}")
  done
  if [ "${#bad[@]}" -gt 0 ]; then
    echo "❌ Python format check failed: ${bad[*]}. Run: ruff format <files>"
    failed=1
  fi
  echo "🔍 Checking Python lint..."
  lint_failed=0
  for i in "${!PATHS[@]}"; do
    git cat-file blob "${BLOBS[$i]}" | ruff check --quiet --stdin-filename "${PATHS[$i]}" - || lint_failed=1
  done
  if [ "$lint_failed" -ne 0 ]; then
    echo "❌ Python lint failed. Run: ruff check --fix <files>"
    failed=1
  fi
fi

# --- Markdown ---------------------------------------------------------------------------------------
# ステージした版を元のパスのまま $work/md に書き出し、そこで markdownlint を走らせる（設定と
# .markdownlintignore はリポジトリの直下のものを写し、相対のパスの書き方をそのまま効かせる）
collect '*.md'
if [ "${#PATHS[@]}" -gt 0 ] && command -v markdownlint >/dev/null 2>&1; then
  echo "🔧 Auto-fixing Markdown lint..."
  md="$work/md"
  mkdir -p "$md"
  for f in .markdownlint.json .markdownlint.jsonc .markdownlint.yaml .markdownlint.yml .markdownlintrc .markdownlintignore; do
    if [ -f "$f" ]; then cp "$f" "$md/$f"; fi
  done
  for i in "${!PATHS[@]}"; do
    mkdir -p "$md/$(dirname "${PATHS[$i]}")"
    git cat-file blob "${BLOBS[$i]}" >"$md/${PATHS[$i]}"
  done
  (cd "$md" && markdownlint --fix -- "${PATHS[@]}" 2>/dev/null) || true
  # markdownlint --fix が直せない MD040（言語指定なし）/ MD060（CJK テーブル整列）を補完する。実体は
  # language-checks スキルの同期先（~/.claude）。無ければ飛ばし、続く検査で残った違反として出る
  MD_FIXER="$HOME/.claude/skills/language-checks/scripts/fix-markdown-lint.py"
  if [ -f "$MD_FIXER" ] && command -v python3 >/dev/null 2>&1; then
    if ! (cd "$md" && python3 "$MD_FIXER" "${PATHS[@]}"); then
      echo "❌ fix-markdown-lint.py failed. Run: python3 $MD_FIXER <files>"
      failed=1
    fi
  fi
  for i in "${!PATHS[@]}"; do
    path=${PATHS[$i]}
    fixed=$(git hash-object -w --path="$path" "$md/$path")
    [ "$fixed" != "${BLOBS[$i]}" ] || continue
    mode=$(GIT_LITERAL_PATHSPECS=1 git ls-files --stage -- "$path" | cut -d' ' -f1)
    git update-index --cacheinfo "$mode,$fixed,$path"
    # ステージしていない編集が無いファイルだけ、作業ツリーにも書く（あれば、作業ツリーは触らない）
    if git cat-file blob "${BLOBS[$i]}" | cmp -s - "$path"; then
      cat "$md/$path" >"$path"
    fi
  done
  echo "🔍 Checking Markdown lint..."
  if ! (cd "$md" && markdownlint -- "${PATHS[@]}" 2>&1); then
    echo "❌ Markdown lint failed (unfixable issues remain)"
    failed=1
  fi
fi

# --- Rust -------------------------------------------------------------------------------------------
# 1 つずつ標準入力で rustfmt に渡し（ファイルで渡すと mod の先を読みにいく）、整えた出力と比べる（標準
# 入力では --check が差を見ない）。edition は最寄りの Cargo.toml のパッケージのもの（cargo metadata が
# workspace からの継承も解く）、クレートの外は 2024。rustfmt.toml は、ファイルのディレクトリから探させる
# 検証: modules/home/parts/tests/pre-commit-rust-format.sh
collect '*.rs'
if [ "${#PATHS[@]}" -gt 0 ] && command -v rustfmt >/dev/null 2>&1; then
  echo "🔍 Checking Rust format..."
  declare -A edition=()
  bad=()
  for i in "${!PATHS[@]}"; do
    path=${PATHS[$i]}
    d=$(dirname "$path")
    m=""
    while :; do
      if [ -f "$d/Cargo.toml" ]; then
        m="$d/Cargo.toml"
        break
      fi
      [ "$d" = "." ] && break
      d=$(dirname "$d")
    done
    ed=2024
    if [ -n "$m" ]; then
      command -v cargo >/dev/null 2>&1 || continue
      if [ -z "${edition[$m]+set}" ]; then
        if ! cargo metadata --no-deps --format-version 1 --manifest-path "$m" >"$work/metadata" 2>"$work/metadata.err"; then
          echo "❌ cargo metadata が失敗しました（$m）:"
          cat "$work/metadata.err"
          failed=1
          continue
        fi
        edition[$m]=$(jq -r --arg m "$(realpath "$m")" '.packages[] | select(.manifest_path == $m) | .edition' "$work/metadata")
      fi
      ed=${edition[$m]}
    fi
    dir=$(dirname "$path")
    [ -d "$dir" ] || dir=.
    git cat-file blob "${BLOBS[$i]}" >"$work/rs"
    if ! (cd "$dir" && rustfmt --edition "$ed" <"$work/rs" 2>/dev/null) | cmp -s - "$work/rs"; then
      bad+=("$path")
    fi
  done
  if [ "${#bad[@]}" -gt 0 ]; then
    echo "❌ Rust format check failed: ${bad[*]}. Run: cargo fmt（クレートの外は rustfmt --edition 2024 <files>）"
    failed=1
  fi
fi

exit "$failed"
