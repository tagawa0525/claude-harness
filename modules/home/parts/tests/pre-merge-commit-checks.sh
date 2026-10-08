#!/usr/bin/env bash
# =============================================================================
# main / master へのマージコミットが、マージ結果に対する品質チェックを通ったときだけ
# 作られることを検証する
# =============================================================================
# ../git.nix の pre-merge-commit は core.hooksPath でグローバル配布され、マージコミットを
# 作る直前（作業ツリーと index がマージ結果になった状態）に run-checks.sh --merge を実行する。
# 競合を解決して git commit（git merge --continue）で締めるときは pre-merge-commit が
# 呼ばれず pre-commit だけが呼ばれるので、pre-commit が MERGE_HEAD を見て同じ検査に回す。
#
# 実物の run-checks.sh（このリポジトリの .claude/skills/language-checks/scripts）を一時 HOME の
# ~/.claude に置き、実物の ruff / markdownlint で判定させる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-merge-commit-checks.sh

set -euo pipefail

HOST="${1:-r995}"
ROOT=$(git rev-parse --show-toplevel)

for tool in ruff markdownlint; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "✗ $tool が PATH にない。run-checks.sh の判定を再現できないので、このテストは成立しない"
    exit 1
  fi
done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
for hook in pre-commit pre-merge-commit; do
  echo "==> $hook hook を取り出す（nixosConfigurations.${HOST}）"
  nix eval --raw --option warn-dirty false \
    "$ROOT#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/$hook\".text" \
    > "$HOOKS/$hook"
  chmod +x "$HOOKS/$hook"
done

export HOME="$WORK/home"
mkdir -p "$HOME/.claude/skills/language-checks"
cp -r "$ROOT/.claude/skills/language-checks/scripts" "$HOME/.claude/skills/language-checks/"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
git config --global core.hooksPath "$HOOKS"
# 検査は Claude Code セッションに限らない。main ガード（GitHub リモートのみ）も対象外
unset CLAUDECODE

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# 使わない import（F401）はどの版の ruff でも既定で落ちる違反
BAD_PY=$'import os\n'
GOOD_PY=$'print("ok")\n'
# 末尾の空白 3 つ（MD009）は markdownlint --fix が直せる違反。マージでは直さずに止める
BAD_MD=$'# Title\n\nSome text.   \n'

# new_repo <dir>: main に shared.txt だけを持つ repo を作る
new_repo() {
  git init -q -b main "$1"
  echo base > "$1/shared.txt"
  git -C "$1" add shared.txt
  git -C "$1" commit -q --no-verify -m "chore: init"
}

# branch_with <repo> <branch> <file> <content>: main から branch を作り、file を足してコミットする。
# 違反を含むファイルも置けるよう pre-commit は飛ばす（マージの検査だけを見る）
branch_with() {
  git -C "$1" switch -q -c "$2" main
  printf '%s' "$4" > "$1/$3"
  git -C "$1" add "$3"
  git -C "$1" commit -q --no-verify -m "feat: $3"
  git -C "$1" switch -q main
}

# run <repo> <command...>: repo で実行し、出力を $WORK/out に残して終了コードを返す
run() {
  local repo="$1" rc=0
  shift
  (cd "$repo" && "$@") >"$WORK/out" 2>&1 </dev/null || rc=$?
  return $rc
}

parents() { git -C "$1" rev-list --parents -n 1 HEAD | wc -w; }

# --- 1. 検査を通るマージ結果: マージコミットが作られる ---------------------------------------
R="$WORK/pass"
new_repo "$R"
branch_with "$R" feat good.py "$GOOD_PY"
if run "$R" git merge --no-ff -m "Merge: good" feat && [ "$(parents "$R")" -eq 3 ] \
  && grep -q "ALL_OK" "$WORK/out"; then
  ok "検査を通るマージ結果は main にマージコミットとして入る"
else
  ng "検査を通るマージ結果なのにマージコミットが作られない（または検査が走っていない）"
fi

# --- 2. 検査に落ちるマージ結果: マージコミットを作らず、直し方を示す -------------------------
R="$WORK/fail"
new_repo "$R"
branch_with "$R" feat bad.py "$BAD_PY"
BEFORE=$(git -C "$R" rev-parse HEAD)
if run "$R" git merge --no-ff -m "Merge: bad" feat; then
  ng "lint に落ちるマージ結果なのに main にマージコミットが作られた"
elif [ "$(git -C "$R" rev-parse HEAD)" != "$BEFORE" ]; then
  ng "マージは失敗扱いなのに HEAD が動いた"
elif grep -q "FAILED: python lint" "$WORK/out" && grep -q "git merge --abort" "$WORK/out"; then
  ok "lint に落ちるマージ結果ではマージコミットを作らず、失敗した検査と直し方を示す"
else
  ng "マージは止まったが、失敗した検査か直し方（git merge --abort）が出力にない"
fi
# 止まったあと git commit で締めようとしても、同じ検査で止まる
if run "$R" git commit --no-edit; then
  ng "pre-merge-commit で止まったマージが git commit で検査なしに締められた"
else
  ok "pre-merge-commit で止まったマージは git commit でも同じ検査で止まる"
fi
git -C "$R" merge --abort

# --- 3. Markdown の違反は直さずに止める（マージコミットに親にない修正を混ぜない） ----------------
R="$WORK/md"
new_repo "$R"
branch_with "$R" feat README.md "$BAD_MD"
if run "$R" git merge --no-ff -m "Merge: md" feat; then
  ng "lint に落ちる Markdown のマージ結果なのにマージコミットが作られた"
else
  staged=$(git -C "$R" show :README.md; printf x); staged=${staged%x}
  if [[ "$staged" == "$BAD_MD" ]]; then
    ok "Markdown の違反はマージで自動修正せずに止める"
  else
    ng "マージの検査が Markdown を書き換えた"
  fi
  git -C "$R" merge --abort
fi

# --- 4. main / master 以外へのマージは検査しない ---------------------------------------------
R="$WORK/other"
new_repo "$R"
branch_with "$R" feat bad.py "$BAD_PY"
git -C "$R" switch -q -c topic main
if run "$R" git merge --no-ff -m "Merge: into topic" feat && [ "$(parents "$R")" -eq 3 ]; then
  ok "main 以外のブランチへのマージは検査しない"
else
  ng "main 以外のブランチへのマージが止められた"
fi

# --- 5. master も main と同じく検査する -------------------------------------------------------
R="$WORK/master"
new_repo "$R"
git -C "$R" branch -q -m main master
git -C "$R" switch -q -c feat master
printf '%s' "$BAD_PY" > "$R/bad.py"
git -C "$R" add bad.py
git -C "$R" commit -q --no-verify -m "feat: bad.py"
git -C "$R" switch -q master
if run "$R" git merge --no-ff -m "Merge: bad" feat; then
  ng "lint に落ちるマージ結果なのに master にマージコミットが作られた"
else
  ok "master へのマージも検査する"
  git -C "$R" merge --abort
fi

# --- 6. fast-forward はマージコミットを作らないので検査しない ---------------------------------
R="$WORK/ff"
new_repo "$R"
branch_with "$R" feat bad.py "$BAD_PY"
if run "$R" git merge --ff-only feat && [ "$(git -C "$R" rev-parse HEAD)" = "$(git -C "$R" rev-parse feat)" ]; then
  ok "git merge --ff-only は検査なしで進む（マージコミットがない）"
else
  ng "fast-forward が止められた"
fi

# --- 7. 競合を解決して締めるときも検査する（pre-commit 経由） ---------------------------------
R="$WORK/conflict"
new_repo "$R"
git -C "$R" switch -q -c feat-bad main
echo theirs > "$R/shared.txt"
printf '%s' "$BAD_PY" > "$R/bad.py"
git -C "$R" add shared.txt bad.py
git -C "$R" commit -q --no-verify -m "feat: bad"
git -C "$R" switch -q -c feat-good main
echo theirs > "$R/shared.txt"
printf '%s' "$GOOD_PY" > "$R/good.py"
git -C "$R" add shared.txt good.py
git -C "$R" commit -q --no-verify -m "feat: good"
git -C "$R" switch -q main
echo ours > "$R/shared.txt"
git -C "$R" commit -q --no-verify -am "chore: ours"
BEFORE=$(git -C "$R" rev-parse HEAD)

run "$R" git merge --no-ff -m "Merge: bad" feat-bad || true
echo resolved > "$R/shared.txt"
git -C "$R" add shared.txt
if run "$R" env GIT_EDITOR=true git merge --continue; then
  ng "競合を解決したマージ結果が lint に落ちるのに git merge --continue でマージコミットが作られた"
elif [ "$(git -C "$R" rev-parse HEAD)" = "$BEFORE" ] && grep -q "FAILED: python lint" "$WORK/out"; then
  ok "競合を解決して git merge --continue で締めるときも検査で止める"
else
  ng "git merge --continue は失敗したが、検査の失敗として止まっていない"
fi
git -C "$R" merge --abort

run "$R" git merge --no-ff -m "Merge: good" feat-good || true
echo resolved > "$R/shared.txt"
# commit -a では hook に渡る index が一時ファイル（index.lock）になる
if run "$R" git commit -a --no-edit && [ "$(parents "$R")" -eq 3 ] && grep -q "ALL_OK" "$WORK/out"; then
  ok "競合を解決して git commit -a で締めるとき、検査を通ればマージコミットが作られる"
else
  ng "競合を解決した検査を通るマージ結果が git commit -a で締められない（または検査が走っていない）"
fi

# --- 8. 作業ツリーにマージ結果以外の変更があれば止める ----------------------------------------
# 検査は作業ツリーで走るので、コミットされない変更があると、マージコミットとは別物を検査してしまう
R="$WORK/dirty"
new_repo "$R"
echo local > "$R/local.txt"
git -C "$R" add local.txt
git -C "$R" commit -q --no-verify -m "chore: local"
branch_with "$R" feat good.py "$GOOD_PY"
echo "uncommitted" > "$R/local.txt"
if run "$R" git merge --no-ff -m "Merge: dirty" feat; then
  ng "作業ツリーに未コミットの変更があるのにマージコミットが作られた"
elif grep -q "local.txt" "$WORK/out" && [ "$(cat "$R/local.txt")" = "uncommitted" ]; then
  ok "作業ツリーにマージ結果以外の変更があれば止め、そのファイルを示す（変更は残す）"
else
  ng "作業ツリーの変更で止まったが、ファイルを示していないか変更が消えた"
fi

# --- 9. fork（upstream リモートあり）では検査しない ---------------------------------------------
R="$WORK/fork"
new_repo "$R"
git -C "$R" remote add upstream https://example.invalid/upstream.git
branch_with "$R" feat bad.py "$BAD_PY"
if run "$R" git merge --no-ff -m "Merge: fork" feat && [ "$(parents "$R")" -eq 3 ] \
  && grep -q "飛ばします" "$WORK/out"; then
  ok "upstream リモートのある fork では検査を飛ばし、その旨を出す"
else
  ng "fork のマージが止められた（または飛ばした旨の出力がない）"
fi

# --- 10. run-checks.sh が無ければ止める（検査できないままマージしない） ------------------------
R="$WORK/missing"
new_repo "$R"
branch_with "$R" feat good.py "$GOOD_PY"
mv "$HOME/.claude" "$HOME/.claude.off"
if run "$R" git merge --no-ff -m "Merge: missing" feat; then
  ng "run-checks.sh が無いのにマージコミットが作られた"
elif grep -q "run-checks.sh" "$WORK/out"; then
  ok "run-checks.sh が無ければマージコミットを作らず、その旨を示す"
else
  ng "run-checks.sh が無くて止まったが、理由が出力にない"
fi
mv "$HOME/.claude.off" "$HOME/.claude"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
