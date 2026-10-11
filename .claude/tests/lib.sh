#!/usr/bin/env bash
# claude-hooks（PreToolUse hook のルール）と .claude/scripts のテスト共通ライブラリ
#
# 各テストファイルから `source "$(dirname "$0")/lib.sh"` で読み込む。
# 実行のたびに一時ディレクトリを HOME として使い、実環境の git 設定・gh 認証・
# ~/.claude を一切参照しない。gh は make_fake_gh で PATH 先頭に偽物を置く。
#
# 使い方: 各テストは `it "説明"` で開始し、assert_* で検証する。
# 失敗しても続行し、最後に finish で件数を報告して失敗があれば exit 1。

set -uo pipefail

CLAUDE_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck disable=SC2034  # scripts/*.sh が使う
SCRIPTS_DIR="$CLAUDE_DIR/scripts"

TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT

export HOME="$TEST_ROOT/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
export GH_CONFIG_DIR="$HOME/.config/gh"
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
git config --global advice.detachedHead false
# hook は Claude Code セッションからの実行を前提にする
export CLAUDECODE=1

PASS=0
FAIL=0
CURRENT=""

it() {
  CURRENT="$1"
}

_fail() {
  FAIL=$((FAIL + 1))
  echo "✗ $CURRENT: $1" >&2
  shift
  for line in "$@"; do echo "    $line" >&2; done
}

_pass() {
  PASS=$((PASS + 1))
  echo "✓ $CURRENT"
}

# assert_eq <expected> <actual>
assert_eq() {
  if [[ "$1" == "$2" ]]; then _pass; else _fail "expected '$1', got '$2'"; fi
}

# assert_ne <unexpected> <actual>
assert_ne() {
  if [[ "$1" != "$2" ]]; then _pass; else _fail "unexpected '$1'"; fi
}

# assert_contains <haystack> <needle>
assert_contains() {
  if [[ "$1" == *"$2"* ]]; then _pass; else _fail "missing '$2' in:" "$1"; fi
}

# assert_not_contains <haystack> <needle>
assert_not_contains() {
  if [[ "$1" != *"$2"* ]]; then _pass; else _fail "unexpected '$2' in:" "$1"; fi
}

# assert_file_exists <path>
assert_file_exists() {
  if [[ -e "$1" ]]; then _pass; else _fail "file not found: $1"; fi
}

# assert_file_missing <path>
assert_file_missing() {
  if [[ ! -e "$1" ]]; then _pass; else _fail "file should not exist: $1"; fi
}

finish() {
  echo
  echo "passed: $PASS, failed: $FAIL"
  (( FAIL == 0 ))
}

# ---------------------------------------------------------------------------
# git フィクスチャ
# ---------------------------------------------------------------------------

# make_repo <dir>: main ブランチに初期コミットを持つリポジトリを作る
make_repo() {
  local dir="$1"
  mkdir -p "$dir"
  git -C "$dir" init -q -b main
  echo "init" > "$dir/README.md"
  git -C "$dir" add README.md
  git -C "$dir" commit -q -m "chore: init"
}

# make_remote <repo> [github]: bare リポジトリを origin として追加し main を送る。
# 第2引数 github を渡すと、github.com の URL を持つ remote（github）も追加する。
# hook / script は「いずれかの remote の URL に github.com が含まれるか」で
# GitHub リモートの有無を判定するので、実際の通信は origin（bare）だけで済む。
# 名前を upstream にしないのは、upstream リモートの有無が「他人のプロジェクトの fork」の
# 判定（git hook と claude-hooks の pre-pr-create-check）に使われるから
make_remote() {
  local repo="$1" kind="${2:-local}"
  local bare="$repo.git"
  git init -q --bare "$bare"
  git -C "$repo" remote add origin "$bare"
  if [[ "$kind" == "github" ]]; then
    git -C "$repo" remote add github "https://github.com/example/$(basename "$repo").git"
  fi
  git -C "$repo" push -q -u origin main
}

# commit_file <repo> <path> <message> [lines]
commit_file() {
  local repo="$1" path="$2" msg="$3" lines="${4:-1}"
  mkdir -p "$repo/$(dirname "$path")"
  seq "$lines" > "$repo/$path"
  git -C "$repo" add "$path"
  git -C "$repo" commit -q -m "$msg"
}

# ---------------------------------------------------------------------------
# 偽 gh
# ---------------------------------------------------------------------------

# make_fake_tool <name> <case-body>: 引数全体（"$*"）で分岐する偽コマンドを PATH 先頭に置く。
# case-body には `"pr view"*) echo ...;;` のような case 節を書く。
# 一致しなければ "fake <name>: unexpected: <args>" を stderr に出して exit 1。
# 呼び出しは $TEST_ROOT/<name>.log に1行ずつ記録される（fake_log <name> で読む）
make_fake_tool() {
  local name="$1" body="$2"
  local bin="$TEST_ROOT/fakebin" log="$TEST_ROOT/$name.log"
  mkdir -p "$bin"
  : > "$log"
  cat > "$bin/$name" <<FAKE
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$log"
case "\$*" in
$body
  *) echo "fake $name: unexpected: \$*" >&2; exit 1 ;;
esac
FAKE
  chmod +x "$bin/$name"
  case ":$PATH:" in
    *":$bin:"*) ;;
    *) export PATH="$bin:$PATH" ;;
  esac
}

# fake_log <name>: 偽コマンドの呼び出し記録
fake_log() {
  cat "$TEST_ROOT/$1.log"
}

# remove_fake_tool <name>: 偽コマンドを PATH から外す（「ツールなし」の状況を作る）
remove_fake_tool() {
  rm -f "$TEST_ROOT/fakebin/$1"
}

# minimal_path: 偽コマンドと基本ツール（git / jq / coreutils 等）だけを含む PATH を返す。
# ホストに実在する ruff 等を見せずに「ツールがない」状況を作るために使う
minimal_path() {
  local bin="$TEST_ROOT/minbin" tool src
  if [[ ! -d "$bin" ]]; then
    mkdir -p "$bin"
    for tool in bash sh env git jq awk sed grep xargs head tail sort uniq cut tr wc cat ls \
                mktemp rm mv cp mkdir dirname basename readlink realpath printf echo test \
                date seq; do
      src=$(command -v "$tool" 2>/dev/null) || continue
      ln -s "$src" "$bin/$tool"
    done
  fi
  echo "$TEST_ROOT/fakebin:$bin"
}

# make_fake_gh <case-body>: gh 用のショートカット。記録は $FAKE_GH_LOG。
# 呼び出しごとの GH_REPO（gh の対象のリポジトリ）も記録する（fake_gh_repos で読む）
make_fake_gh() {
  : > "$TEST_ROOT/gh-repo.log"
  make_fake_tool gh "  *) printf '%s\\n' \"\${GH_REPO-(unset)}\" >> \"$TEST_ROOT/gh-repo.log\" ;;&
$1"
  export FAKE_GH_LOG="$TEST_ROOT/gh.log"
}

# fake_gh_repos: 偽 gh が受けた GH_REPO の値（重複を除く）
fake_gh_repos() {
  sort -u "$TEST_ROOT/gh-repo.log"
}

# ---------------------------------------------------------------------------
# hook 実行
# ---------------------------------------------------------------------------

# hook_input <command> [run_in_background]: Bash ツールの PreToolUse の入力 JSON（cwd は今のディレクトリ）
hook_input() {
  local command="$1" bg="${2:-false}"
  jq -n --arg cmd "$command" --argjson bg "$bg" \
    '{hook_event_name: "PreToolUse", tool_name: "Bash", cwd: env.PWD,
      tool_input: {command: $cmd, run_in_background: $bg}}'
}

# run_hook の失敗で、テストの全体を止める（run_hook は $(...) の中で呼ばれ、exit では呼び出し元を止められない）
trap 'exit 1' USR1

# run_hook <rule> <command> [run_in_background]（hooks.sh が CLAUDE_HOOKS_BIN を用意する）
# そのルールだけを評価した stdout を返す。claude-hooks が 0 以外で終わるか、標準エラーに何か出したら
# （知らないルール、拾われた panic）、空の出力を allow と読まずに、テストの全体を止める
run_hook() {
  local hook="$1" command="$2" bg="${3:-false}" input out status=0
  input=$(hook_input "$command" "$bg") || {
    echo "hook の入力を組めません: $command" >&2
    kill -USR1 "$$"
    exit 1
  }
  out=$("$CLAUDE_HOOKS_BIN" pre-tool-use --rule "$hook" <<<"$input" 2>"$TEST_ROOT/hook-stderr") || status=$?
  if ((status != 0)) || [[ -s "$TEST_ROOT/hook-stderr" ]]; then
    echo "claude-hooks が失敗しました（--rule $hook、終了コード $status）: $command" >&2
    cat "$TEST_ROOT/hook-stderr" >&2
    kill -USR1 "$$"
    exit 1
  fi
  [[ -z "$out" ]] || printf '%s\n' "$out"
}

# decision <hook-output> [no-decision] → permissionDecision（出力が空なら "allow"。出力に判定が無ければ
# <no-decision>（既定 "allow"）。additionalContext だけの出力を見分けるときに "context" を渡す）
decision() {
  if [[ -z "$1" ]]; then
    echo allow
  else
    jq -r --arg none "${2:-allow}" '.hookSpecificOutput.permissionDecision // $none' <<<"$1"
  fi
}

# reason <hook-output> → permissionDecisionReason
reason() {
  [[ -z "$1" ]] && return 0
  jq -r '.hookSpecificOutput.permissionDecisionReason // ""' <<<"$1"
}

# additional_context <hook-output>
additional_context() {
  [[ -z "$1" ]] && return 0
  jq -r '.hookSpecificOutput.additionalContext // ""' <<<"$1"
}
