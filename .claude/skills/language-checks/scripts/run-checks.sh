#!/usr/bin/env bash
# run-checks.sh — プロジェクトの言語を検出し、フォーマット → リント → テストを実行する
#
# Usage: run-checks.sh [-C <dir>] [--merge] [--base <ref>]
#
#   --merge  マージ結果を検査する（git の pre-merge-commit hook が使う）。「ステージ済み」の代わりに
#            HEAD（マージ先）と作業ツリーの差分で言語と対象を決め、自動修正はしない
#            （直した内容はどちらの親にもない変更としてマージコミットに紛れ込むため、lint で止める）。
#            commit -a でマージを締めるとき hook に渡る index は一時ファイルなので、index に頼らない
#   -C <dir>  <dir> の worktree を検査する（git -C と同じ。今いる場所と別の worktree のブランチを検査するとき）
#   --base <ref>  コミット済みのブランチを検査する（/git-merge がマージの前に実行する）。<ref> との分岐点から
#            作業ツリーまでの差分で言語と対象を決め、--merge と同じく自動修正はしない。
#            ステージ済みの差分は、クリーンなブランチでは空なので使えない
#
# 言語の検出（language-checks スキルの規約。いずれかを満たせば対象）:
#   Rust:     Cargo.toml がある、または .rs がステージ済み。ルートに Cargo.toml が無ければ、
#             ステージ済みの .rs から最も近い Cargo.toml のディレクトリ（crate）ごとに実行する。
#             ルートに Cargo.toml と scripts/run-rust-docs-tests.sh（実行権限が要る）があれば、差分が .md だけなら
#             fmt、clippy、cargo test の代わりにそのスクリプトだけを、ほかの差分では全体の後にそのスクリプトも実行する
#   Python:   pyproject.toml / setup.py / requirements.txt がある、または .py がステージ済み。テストは
#             scripts/run-python-tests.sh があればそれ、なければ tests/ かルートの test_*.py を pytest で、
#             それも無ければ追跡している test_*.py / *_test.py のあるディレクトリごとに
#   Nix:      flake.nix がある、または .nix がステージ済み
#   Markdown: .md がステージ済み（staged-checks markdown で、ステージした版を直して検査する。--merge では検査だけ）。.pre-commit-config.yaml で design-records に
#             オプトインしていれば、.md の変更が無くても design-records check
#   Shell:    .sh がステージ済み（ステージ済みのファイルだけを shellcheck で検査）
#
# 最初に失敗したチェックで止まり、失敗したコマンドと自動修正コマンド（あれば）を出す。
# ツールがなければ SKIP を出して続行する。
#
# 出力:
#   RUN: <lang> <stage>: <command>
#   OK: <lang> <stage> | SKIP: <lang> <stage> (<理由>) | FAILED: <lang> <stage>: <command>
#   FIX: <自動修正コマンド>（FAILED のときのみ、あれば）
#   ALL_OK（すべて通ったとき）
#
# プロジェクトの CLAUDE.md にチェックコマンドが明記されていればそちらを優先する
# （例: feature gate のあるクレートでは --all-features が必要）。ここでの既定値は汎用。

set -euo pipefail

MERGE=0
BASE_REF=""
DIR=""
while (( $# > 0 )); do
  case "$1" in
    --merge) MERGE=1 ;;
    --base | -C)
      if (( $# < 2 )); then
        echo "ERROR: $1 には引数が要ります" >&2
        exit 2
      fi
      if [[ "$1" == "-C" ]]; then DIR="$2"; else BASE_REF="$2"; MERGE=1; fi
      shift
      ;;
    *)
      echo "ERROR: 知らないオプション: $1（Usage: run-checks.sh [-C <dir>] [--merge] [--base <ref>]）" >&2
      exit 2
      ;;
  esac
  shift
done

if [[ -n "$DIR" ]] && ! cd -- "$DIR" 2>/dev/null; then
  echo "ERROR: -C $DIR に移れません" >&2
  exit 2
fi
ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "ERROR: git リポジトリ内で実行してください" >&2
  exit 1
}
cd "$ROOT" || exit 1

DIFF_BASE=HEAD
if [[ -n "$BASE_REF" ]] && ! DIFF_BASE=$(git merge-base "$BASE_REF" HEAD 2>/dev/null); then
  echo "ERROR: --base $BASE_REF と HEAD の分岐点を求められません" >&2
  exit 2
fi

# diff_names [git diff の追加引数...]: 検査対象の差分のファイル名。通常はステージ済み、
# --merge では HEAD と作業ツリーの差分、--base では分岐点と作業ツリーの差分
diff_names() {
  if (( MERGE )); then
    git diff "$DIFF_BASE" --name-only "$@"
  else
    git diff --cached --name-only "$@"
  fi
}
# changed_files [git diff の追加引数...]: 検査対象の変更ファイル（追加、変更、rename の後の名前）
changed_files() {
  diff_names --diff-filter=ACMR "$@"
}

staged_has() {
  changed_files -- "$1" | grep -q .
}

# run_stage <lang> <stage> <tool> <fix-command|""> <command...>
# command は関数名でもよい（パイプラインを含むチェック用）
run_stage() {
  local lang="$1" stage="$2" tool="$3" fix="$4"
  shift 4
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "SKIP: $lang $stage ($tool not found)"
    return 0
  fi
  echo "RUN: $lang $stage: $*"
  if "$@"; then
    echo "OK: $lang $stage"
  else
    echo "FAILED: $lang $stage: $*"
    [[ -n "$fix" ]] && echo "FIX: $fix"
    exit 1
  fi
}

DETECTED=0

# 排他の Slurm のジョブが実行中は、cargo と nix flake check を 1 コアに限る。全コアを使うとそのジョブの時間の
# 記録を乱し、止めるとジョブの間（数時間）マージできないため、止めずに遅く実行する。
# claude-hooks の guard-exclusive-slurm は、このスクリプトを対象にしない
CARGO_PIN=()
CARGO_JOBS=()
NIX_LIMIT=()
# shellcheck source=../../../scripts/lib/slurm-exclusive.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/slurm-exclusive.sh"
if EXCLUSIVE_JOB=$(exclusive_slurm_job); then
  echo "排他のジョブ ${EXCLUSIVE_JOB% *}（${EXCLUSIVE_JOB##* }）が実行中なので 1 コアで実行する（遅い）"
  CARGO_PIN=(taskset -c "$(( $(getconf _NPROCESSORS_ONLN) - 1 ))")
  CARGO_JOBS=(-j 1)
  NIX_LIMIT=(--cores 1 -j 1)
fi

# --- Rust ---
# rust_targets: ステージ済みの .rs ごとに、上へたどって最も近い Cargo.toml のディレクトリ（crate）を
# "crate <dir>" で出す。crate に属さない .rs は "stray <file>" で出す
rust_targets() {
  local f dir
  while IFS= read -r -d '' f; do
    dir=$(dirname -- "$f")
    while [[ "$dir" != . && ! -f "$dir/Cargo.toml" ]]; do
      dir=$(dirname -- "$dir")
    done
    if [[ -f "$dir/Cargo.toml" ]]; then
      echo "crate $dir"
    else
      echo "stray $f"
    fi
  done < <(changed_files -z -- '*.rs')
}
# cargo_in <dir> <args...>: crate のディレクトリで cargo を実行する
cargo_in() {
  (cd -- "$1" && shift && "${CARGO_PIN[@]}" cargo "$@")
}
run_rust() {
  local dir="$1"
  # 修正コマンドはコピーしてそのまま実行できるよう、空白や - で始まるパスも引用する
  run_stage rust format cargo "(cd -- $(printf '%q' "$dir") && cargo fmt)" cargo_in "$dir" fmt --check
  run_stage rust lint cargo "" cargo_in "$dir" clippy "${CARGO_JOBS[@]}" --all-targets -- -D warnings
  run_stage rust test cargo "" cargo_in "$dir" test "${CARGO_JOBS[@]}"
}
# docs_only_diff: 差分が空でなく、すべて .md か。削除と rename の前後も数える（--diff-filter=ACMR の
# changed_files では、.rs の削除や .rs から .md への rename が見えない）。.md 以外は、.rs と Cargo.toml に限らず
# clippy.toml やテストが読むデータも Rust の結果を変えうるので、すべて全体を実行する側に数える
docs_only_diff() {
  local all rest
  if ! all=$(diff_names --no-renames) || ! rest=$(diff_names --no-renames -- ':(exclude)*.md'); then
    echo "FAILED: rust docs-test: 差分（git diff）を読めません"
    exit 1
  fi
  [[ -n "$all" && -z "$rest" ]]
}
# 文書の差分で実行するテストの宣言。文書を読む Rust のテスト（文書どうしの整合など）を持つリポジトリが置き、
# 差分が .md だけのときは fmt、clippy、全体の cargo test の代わりにこれを実行する（scripts/run-python-tests.sh と同じ形）。
# ほかの差分では全体の後にも実行する。宣言の外の変更（Cargo.toml のパッケージ名など）で壊れた宣言が、次の文書だけの
# 差分で初めて落ちないようにするため。同じ理由で、実行権限も差分によらず確かめる
RUST_DOCS_TESTS=scripts/run-rust-docs-tests.sh
run_rust_docs_tests() {
  run_stage rust docs-test bash "" "${CARGO_PIN[@]}" bash "$RUST_DOCS_TESTS"
}
if [[ -f Cargo.toml && -e "$RUST_DOCS_TESTS" ]]; then
  DETECTED=1
  if [[ ! -x "$RUST_DOCS_TESTS" ]]; then
    echo "FAILED: rust docs-test: $RUST_DOCS_TESTS に実行権限が無い（chmod +x）"
    exit 1
  fi
  if docs_only_diff; then
    for stage in format lint test; do
      echo "SKIP: rust $stage (差分が .md だけなので $RUST_DOCS_TESTS だけを実行する)"
    done
  else
    run_rust .
  fi
  run_rust_docs_tests
elif [[ -f Cargo.toml ]]; then
  DETECTED=1
  run_rust .
elif staged_has '*.rs'; then
  DETECTED=1
  mapfile -t targets < <(rust_targets | sort -u)
  stray=()
  for t in "${targets[@]}"; do
    case "$t" in
      "crate "*) run_rust "${t#crate }" ;;
      "stray "*) stray+=("${t#stray }") ;;
    esac
  done
  if (( ${#stray[@]} > 0 )); then
    echo "SKIP: rust (Cargo.toml の無い場所の .rs: ${stray[*]})"
  fi
fi

# --- Python ---
# python_test_files: git が追跡している test_*.py / *_test.py（サブディレクトリを含む。無視したディレクトリや
# references/ は追跡されないので入らない）
python_test_files() {
  git ls-files -- ':(glob)**/test_*.py' ':(glob)**/*_test.py'
}
# python_subdir_tests <file>...: 追跡しているテストを、ディレクトリごとに実行する。テストは隣の .py を import するので、
# そのディレクトリを起点にする。ディレクトリやパターンでなく追跡しているファイルだけを渡すのは、子のディレクトリの
# テストを親と子で二重に実行せず、追跡していないファイルを拾わないため。pytest が python3 から使えれば pytest、
# 無ければ標準の unittest（pytest を足さない環境で、テストが走らないまま通るのを避ける）
python_subdir_tests() {
  local f dir name pytest_ok=0
  local -a dirs files cmd
  python3 -m pytest --version >/dev/null 2>&1 && pytest_ok=1
  # スラッシュの無いパス（ルート直下）は . とみなす
  mapfile -t dirs < <(for f in "$@"; do [[ "$f" == */* ]] && echo "${f%/*}" || echo .; done | sort -u)
  for dir in "${dirs[@]}"; do
    files=()
    for f in "$@"; do
      if [[ "$f" == */* ]]; then
        [[ "${f%/*}" == "$dir" ]] && files+=("${f##*/}")
      else
        [[ "$dir" == . ]] && files+=("$f")
      fi
    done
    if (( pytest_ok )); then
      cmd=(python3 -m pytest "${files[@]}")
    else
      cmd=(python3 -m unittest)
      for name in "${files[@]}"; do cmd+=("${name%.py}"); done
    fi
    echo "RUN: python test: cd $(printf '%q' "$dir") && ${cmd[*]}"
    (cd -- "$dir" && "${cmd[@]}") || {
      echo "FAILED: python test: cd $(printf '%q' "$dir") && ${cmd[*]}"
      return 1
    }
  done
}
if [[ -f pyproject.toml || -f setup.py || -f requirements.txt ]] || staged_has '*.py'; then
  DETECTED=1
  run_stage python format ruff "ruff format ." ruff format --check .
  run_stage python lint ruff "ruff check --fix ." ruff check .
  if [[ -e scripts/run-python-tests.sh && ! -x scripts/run-python-tests.sh ]]; then
    echo "FAILED: python test: scripts/run-python-tests.sh に実行権限が無い（chmod +x）"
    exit 1
  elif [[ -e scripts/run-python-tests.sh ]]; then
    # リポジトリ固有の実行方法（devShell を分けるなど）
    run_stage python test bash "" bash scripts/run-python-tests.sh
  elif [[ -d tests ]] || compgen -G 'test_*.py' >/dev/null || compgen -G '*_test.py' >/dev/null; then
    run_stage python test pytest "" pytest
  elif listing=$(python_test_files) && [[ -n "$listing" ]]; then
    mapfile -t test_files <<<"$listing"
    run_stage python test python3 "" python_subdir_tests "${test_files[@]}"
  else
    echo "SKIP: python test (テストが見つからない)"
  fi
fi

# --- Nix ---
nix_fmt_check() {
  git ls-files -z '*.nix' | xargs -0 -r nixfmt --check
}
if [[ -f flake.nix ]] || staged_has '*.nix'; then
  DETECTED=1
  run_stage nix format nixfmt "git ls-files -z '*.nix' | xargs -0 -r nixfmt" nix_fmt_check
  run_stage nix lint statix "statix fix" statix check
  if [[ -f flake.nix ]]; then
    run_stage nix test nix "" nix flake check "${NIX_LIMIT[@]}"
  else
    echo "SKIP: nix test (flake.nix がない)"
  fi
fi

# --- Markdown（ステージ済みのみ）---
staged_md() {
  changed_files -z -- '*.md'
}
md_lint() {
  staged_md | xargs -0 -r markdownlint --
}
# design_records_check: ADR の形と ADR どうしの参照。変更したファイルを全部渡し、置き換えられた ADR への
# 参照をそこから探させる（prek の設定の types: [text] と同じ。テキストでないファイルは design-records が読み飛ばす）。
# --changed で、触っていない ADR の推奨の節の warning を出さない。
# xargs で渡さないのは、引数が多いと分けて呼ばれ、ADR どうしの検査の指摘が重なるため。
# プロセス置換は git diff の失敗を返さないので、先に一度実行して失敗を止める
design_records_check() {
  local files
  changed_files >/dev/null || return 1
  mapfile -d '' -t files < <(changed_files -z)
  design-records check --changed "${files[@]}"
}
if staged_has '*.md'; then
  DETECTED=1
  if (( MERGE )); then
    echo "SKIP: markdown format (--merge では自動修正しない)"
    run_stage markdown lint markdownlint "" md_lint
  else
    # ステージした版を直してステージし直し、検査する（グローバルの pre-commit と同じ staged-checks。作業ツリーを
    # 書き換えないので、一部だけステージした .md のステージしていない編集をステージしない）
    run_stage markdown staged staged-checks "" staged-checks markdown
  fi
fi
# design-records は .pre-commit-config.yaml でオプトインしたリポジトリだけで実行する（docs/adr の有無で
# 決めると、別の形の ADR を持つ他人のプロジェクトでも止まる）。.md の変更が無くても実行し、ADR を消すだけの変更でも、
# 消した ADR を指す参照が残っていないかを見る（prek の設定の always_run と同じ）
# grep の 1（一致なし）だけをオプトインしていないと見なし、読めないなどの失敗（2 以上）は止める
DESIGN_RECORDS=0
if [[ -f .pre-commit-config.yaml ]]; then
  rc=0
  grep -Eq "^[[:space:]]*-?[[:space:]]*id:[[:space:]]*[\"']?design-records[\"']?[[:space:]]*(#.*)?\$" \
    .pre-commit-config.yaml || rc=$?
  case "$rc" in
    0) DESIGN_RECORDS=1 ;;
    1) ;;
    *)
      echo "FAILED: markdown design-records: .pre-commit-config.yaml を読めません（grep の終了コード $rc）"
      exit 1
      ;;
  esac
fi
if (( DESIGN_RECORDS )); then
  DETECTED=1
  run_stage markdown design-records design-records "" design_records_check
fi

# --- Shell（ステージ済みのみ）---
sh_lint() {
  changed_files -z -- '*.sh' | xargs -0 -r shellcheck -S warning --
}
if staged_has '*.sh'; then
  DETECTED=1
  run_stage shell lint shellcheck "" sh_lint
fi

if (( DETECTED == 0 )); then
  echo "NOTE: 対象言語が見つかりません（Cargo.toml / pyproject.toml / flake.nix、またはステージ済みの .rs / .py / .nix / .md / .sh）"
  exit 0
fi

echo "ALL_OK"
