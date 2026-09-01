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
# 第2引数 github を渡すと URL を github.com 風にし、insteadOf で bare に向ける
# （`git remote -v` で GitHub リモートと判定させるため）
make_remote() {
  local repo="$1" kind="${2:-local}"
  local bare="$repo.git"
  git init -q --bare "$bare"
  local url="$bare"
  if [[ "$kind" == "github" ]]; then
    url="https://github.com/example/$(basename "$repo").git"
    git config --global "url.$bare.insteadOf" "$url"
  fi
  git -C "$repo" remote add origin "$url"
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

# make_fake_gh <case-body>: 引数全体（"$*"）で分岐する gh を PATH 先頭に置く。
# case-body には `"pr view"*) echo ...;;` のような case 節を書く。
# 一致しなければ "fake gh: unexpected: <args>" を stderr に出して exit 1。
# 呼び出しは $FAKE_GH_LOG に1行ずつ記録される
make_fake_gh() {
  local bin="$TEST_ROOT/fakebin"
  mkdir -p "$bin"
  export FAKE_GH_LOG="$TEST_ROOT/gh.log"
  : > "$FAKE_GH_LOG"
  cat > "$bin/gh" <<FAKE
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$FAKE_GH_LOG"
case "\$*" in
$1
  *) echo "fake gh: unexpected: \$*" >&2; exit 1 ;;
esac
FAKE
  chmod +x "$bin/gh"
  case ":$PATH:" in
    *":$bin:"*) ;;
    *) export PATH="$bin:$PATH" ;;
  esac
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
