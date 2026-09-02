#!/usr/bin/env bash
# =============================================================================
# main 直接コミットを止める 2 つの層が同じ判定になることを検証する
# =============================================================================
# 同じルールを次の 2 か所で守っている:
#   1. .claude/hooks/block-main-commit.sh   Claude Code の PreToolUse hook
#   2. ../git.nix の pre-commit hook        git 側（core.hooksPath でグローバル配布）
#
# 片方だけが塞ぐと作業が止まる。実際に PR #165 で、flake.lock の例外が git 側にだけ
# あって PreToolUse hook になく、nix-rebuild update がコミットできなくなった。
# 例外を足すときは必ず両方に足す。このテストがその一致を確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/main-commit-gates.sh

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
HOOK="$ROOT/.claude/hooks/block-main-commit.sh"

HOST="${1:-r995}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> pre-commit hook を取り出す（nixosConfigurations.${HOST}）"
nix eval --raw --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".text" \
  > "$WORK/pre-commit"
chmod +x "$WORK/pre-commit"

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
export CLAUDECODE=1

PASS=0
FAIL=0

# setup_repo <dir> <github|local>
setup_repo() {
  local dir="$1" kind="$2"
  git init -q -b main "$dir"
  echo init > "$dir/README.md"
  git -C "$dir" add README.md
  git -C "$dir" -c core.hooksPath=/dev/null commit -q -m "chore: init"
  echo '{"nodes":{}}' > "$dir/flake.lock"
  echo x > "$dir/other.txt"
  git -C "$dir" add flake.lock other.txt
  git -C "$dir" -c core.hooksPath=/dev/null commit -q -m "chore: files"
  if [[ "$kind" == "github" ]]; then
    git -C "$dir" remote add origin "https://github.com/example/$(basename "$dir").git"
  fi
}

# check <期待 allow|block> <説明> <repo>
# 2 つの層をそれぞれ実行し、期待と一致し、かつ互いに一致することを確かめる
check() {
  local expect="$1" desc="$2" repo="$3"
  local pre_result hook_result out

  if (cd "$repo" && "$WORK/pre-commit" >/dev/null 2>&1); then
    pre_result=allow
  else
    pre_result=block
  fi

  out=$(cd "$repo" && jq -n '{hook_event_name: "PreToolUse", tool_name: "Bash",
        tool_input: {command: "git commit -m msg"}}' | "$HOOK")
  if [[ -z "$out" ]] || [[ "$(jq -r '.hookSpecificOutput.permissionDecision // "allow"' <<<"$out")" == "allow" ]]; then
    hook_result=allow
  else
    hook_result=block
  fi

  if [[ "$pre_result" == "$expect" && "$hook_result" == "$expect" ]]; then
    PASS=$((PASS + 1))
    echo "✓ $desc（両層とも $expect）"
  else
    FAIL=$((FAIL + 1))
    echo "✗ $desc" >&2
    echo "    期待: $expect / pre-commit: $pre_result / PreToolUse hook: $hook_result" >&2
  fi
}

echo "==> シナリオごとに 2 つの層の判定を突き合わせる"

R="$WORK/gh"; setup_repo "$R" github
echo change > "$R/other.txt"; git -C "$R" add other.txt
check block "GitHubリモートあり・main・通常の変更" "$R"

git -C "$R" restore --staged other.txt; git -C "$R" checkout -q other.txt
echo '{"nodes":{"a":1}}' > "$R/flake.lock"; git -C "$R" add flake.lock
check allow "GitHubリモートあり・main・flake.lock のみ" "$R"

echo change > "$R/other.txt"; git -C "$R" add other.txt
check block "GitHubリモートあり・main・flake.lock と他ファイル" "$R"

git -C "$R" switch -q -c feat/x
check allow "GitHubリモートあり・feature ブランチ" "$R"

L="$WORK/local"; setup_repo "$L" local
echo change > "$L/other.txt"; git -C "$L" add other.txt
check allow "リモートなし・main・通常の変更（PRフロー適用外）" "$L"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
