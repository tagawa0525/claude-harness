#!/usr/bin/env bash
# .claude/hooks と .claude/scripts のテスト共通ライブラリ
#
# 各テストファイルから `source "$(dirname "$0")/lib.sh"` で読み込む。
# 実行のたびに一時ディレクトリを HOME として使い、実環境の git 設定・gh 認証・
# ~/.claude を一切参照しない。gh は make_fake_gh で PATH 先頭に偽物を置く。
#
# 使い方: 各テストは `it "説明"` で開始し、assert_* で検証する。
# 失敗しても続行し、最後に finish で件数を報告して失敗があれば exit 1。

set -uo pipefail

CLAUDE_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
HOOKS_DIR="$CLAUDE_DIR/hooks"
# shellcheck disable=SC2034  # scripts.sh が使う
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
# 第2引数 github を渡すと、github.com の URL を持つ remote（upstream）も追加する。
# hook / script は「いずれかの remote の URL に github.com が含まれるか」で
# GitHub リモートの有無を判定するので、実際の通信は origin（bare）だけで済む
make_remote() {
  local repo="$1" kind="${2:-local}"
  local bare="$repo.git"
  git init -q --bare "$bare"
  git -C "$repo" remote add origin "$bare"
  if [[ "$kind" == "github" ]]; then
    git -C "$repo" remote add upstream "https://github.com/example/$(basename "$repo").git"
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

# make_fake_gh <case-body>: gh 用のショートカット。記録は $FAKE_GH_LOG
make_fake_gh() {
  make_fake_tool gh "$1"
  export FAKE_GH_LOG="$TEST_ROOT/gh.log"
}

# ---------------------------------------------------------------------------
# hook 実行
# ---------------------------------------------------------------------------

# run_hook <hook-file> <command> [run_in_background]
# PreToolUse の入力 JSON を組み立てて hook を実行し、stdout を返す
run_hook() {
  local hook="$1" command="$2" bg="${3:-false}"
  jq -n --arg cmd "$command" --argjson bg "$bg" \
    '{hook_event_name: "PreToolUse", tool_name: "Bash", cwd: env.PWD,
      tool_input: {command: $cmd, run_in_background: $bg}}' \
    | "$HOOKS_DIR/$hook"
}

# decision <hook-output> → permissionDecision（出力が空なら "allow"）
decision() {
  if [[ -z "$1" ]]; then echo allow; else jq -r '.hookSpecificOutput.permissionDecision // "allow"' <<<"$1"; fi
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
