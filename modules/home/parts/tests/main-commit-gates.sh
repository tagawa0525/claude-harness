#!/usr/bin/env bash
# =============================================================================
# main 直接コミットを止める 2 つの層が同じ判定になることを検証する
# =============================================================================
# 同じルールを次の 2 か所で守っている:
#   1. ../claude-hooks の block-main-commit   Claude Code の PreToolUse hook（Rust 製バイナリ）
#   2. ../git-hooks.nix の pre-commit hook        git 側（core.hooksPath でグローバル配布）
#
# 片方だけが塞ぐと作業が止まる。実際に PR #165 で、flake.lock の例外が git 側にだけ
# あって PreToolUse hook になく、nix-rebuild update がコミットできなくなった。
# 例外を足すときは必ず両方に足す。このテストがその一致を確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/main-commit-gates.sh

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
# PreToolUse hook バイナリ。CLAUDE_HOOKS_BIN（絶対パス）で差し替え可（既定は nix build の成果物）
HOOK_BIN="${CLAUDE_HOOKS_BIN:-$(nix build --no-link --print-out-paths --option warn-dirty false "$ROOT#claude-hooks")/bin/claude-hooks}"
# check() は一時のリポジトリへ cd してから呼ぶので、相対のパスは解けない
if [[ "$HOOK_BIN" != /* ]]; then
  echo "CLAUDE_HOOKS_BIN は絶対パスで渡してください: $HOOK_BIN" >&2
  exit 1
fi
# 引数なしで呼ぶと使い方（rules: …）を出し、終了コード 2 で終わる。-x だけではディレクトリや
# 起動できないバイナリを通す
usage=$("$HOOK_BIN" 2>&1 </dev/null) || true
if ! grep -q '^rules: ' <<<"$usage"; then
  echo "claude-hooks を実行できません: $HOOK_BIN" >&2
  echo "$usage" >&2
  exit 1
fi

HOST="${1:-r995}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> pre-commit hook をビルドする（nixosConfigurations.${HOST}）"
cp "$(nix build --no-link --print-out-paths --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".source")" \
  "$WORK/pre-commit"
chmod +x "$WORK/pre-commit"
# MERGE_HEAD があると pre-commit は隣の pre-merge-commit（マージ結果のチェック）を呼ぶ。ここで比べるのは main の
# ガードの判定だけなので、何もせず通す代わりを置く
printf '#!/usr/bin/env bash\nexit 0\n' > "$WORK/pre-merge-commit"
chmod +x "$WORK/pre-merge-commit"

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

# setup_repo <dir> <github|local|gitlab>
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
  elif [[ "$kind" == "gitlab" ]]; then
    git -C "$dir" remote add origin "https://gitlab.example.com/example/$(basename "$dir").git"
  fi
}

# stage_merge <repo>: main の上で別のブランチを --no-commit でマージし、MERGE_HEAD のある状態にする
# （/git-merge で本文の語に拒まれた後や、競合を解決した後の、マージを締める git commit の前の状態）
stage_merge() {
  local dir="$1"
  git -C "$dir" switch -q -c side
  echo side > "$dir/side.txt"
  git -C "$dir" add side.txt
  git -C "$dir" -c core.hooksPath=/dev/null commit -q -m "feat: side"
  git -C "$dir" switch -q main
  git -C "$dir" -c core.hooksPath=/dev/null merge -q --no-ff --no-commit side >/dev/null 2>&1
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

  # 正しく呼べば claude-hooks は 0 で終わり、標準エラーにも systemMessage にも何も出さない。知らないルール
  # （使い方を出して 2）、拾われた panic（標準エラーと 0）、判定できなかった理由（systemMessage。ADR-0033）、
  # 1 つの JSON の object でない出力は、判定が無く allow に見えるので、ここで止める
  local input status=0
  input=$(jq -n '{hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: {command: "git commit -m msg"}}')
  out=$(cd "$repo" && "$HOOK_BIN" pre-tool-use --rule block-main-commit <<<"$input" 2>"$WORK/hook-stderr") ||
    status=$?
  if ((status != 0)) || [[ -s "$WORK/hook-stderr" ]] ||
    { [[ -n "$out" ]] && ! jq -se 'length == 1 and (.[0] | type == "object" and (has("systemMessage") | not))' <<<"$out" >/dev/null; }; then
    echo "claude-hooks が失敗しました（--rule block-main-commit、終了コード $status）: $desc" >&2
    cat "$WORK/hook-stderr" >&2
    [[ -z "$out" ]] || printf '%s\n' "$out" >&2
    exit 1
  fi
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

git -C "$R" restore -q --staged --worktree -- .
git -C "$R" switch -q main
stage_merge "$R"
check allow "GitHubリモートあり・main・マージを締めるコミット（MERGE_HEAD あり）" "$R"
git -C "$R" merge --abort

L="$WORK/local"; setup_repo "$L" local
echo change > "$L/other.txt"; git -C "$L" add other.txt
check block "リモートなし・main・通常の変更" "$L"

git -C "$L" restore --staged other.txt; git -C "$L" checkout -q other.txt
echo '{"nodes":{"a":1}}' > "$L/flake.lock"; git -C "$L" add flake.lock
check allow "リモートなし・main・flake.lock のみ" "$L"
git -C "$L" restore --staged flake.lock; git -C "$L" checkout -q flake.lock

check block "リモートなし・main・何もステージしていない" "$L"

stage_merge "$L"
check allow "リモートなし・main・マージを締めるコミット（MERGE_HEAD あり）" "$L"
git -C "$L" merge --abort

git -C "$L" switch -q -c feat/x
echo change > "$L/other.txt"; git -C "$L" add other.txt
check allow "リモートなし・feature ブランチ" "$L"

# 拒否の文言: どちらの層も、cd の失敗で別のリポジトリにいる場合に気づけるよう、対象のディレクトリを確かめる
# 修正方法を示す（cd が失敗した形では、PreToolUse の層は対象を誤り、git の層だけが止めることがある）
git -C "$L" switch -q main
echo change2 > "$L/other.txt"; git -C "$L" add other.txt
pre_out=$(cd "$L" && "$WORK/pre-commit" 2>&1) || true
hook_out=$(cd "$L" && "$HOOK_BIN" pre-tool-use --rule block-main-commit \
  <<<"$(jq -n '{hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: {command: "git commit -m msg"}}')" |
  jq -r '.hookSpecificOutput.permissionDecisionReason // ""')
for layer in pre-commit PreToolUse; do
  if [[ $layer == pre-commit ]]; then text=$pre_out; else text=$hook_out; fi
  if [[ "$text" == *"対象のディレクトリを確かめる"* && "$text" == *"/git-branch"* ]]; then
    PASS=$((PASS + 1))
    echo "✓ 拒否の文言が対象のディレクトリの確認とブランチの作成を示す（$layer）"
  else
    FAIL=$((FAIL + 1))
    echo "✗ 拒否の文言が対象のディレクトリの確認とブランチの作成を示さない（$layer）: $text" >&2
  fi
done
git -C "$L" restore -q --staged --worktree -- other.txt

O="$WORK/gitlab"; setup_repo "$O" gitlab
echo change > "$O/other.txt"; git -C "$O" add other.txt
check block "GitHub 以外のリモートだけ・main・通常の変更" "$O"

M="$WORK/master"; setup_repo "$M" local
git -C "$M" branch -q -m main master
echo change > "$M/other.txt"; git -C "$M" add other.txt
check block "リモートなし・master・通常の変更" "$M"

U="$WORK/unborn"
git init -q -b main "$U"
echo first > "$U/first.txt"; git -C "$U" add first.txt
check allow "リモートなし・main・HEAD の無いリポジトリの最初のコミット" "$U"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
