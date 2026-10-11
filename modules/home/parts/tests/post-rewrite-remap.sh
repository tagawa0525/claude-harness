#!/usr/bin/env bash
# =============================================================================
# git post-rewrite hook が、オプトインしたリポジトリでだけ、rebase と amend の後に
# 文書の中のコミットの番号を付け直すことを検証する
# =============================================================================
# ../git.nix の post-rewrite は core.hooksPath でグローバル配布され、`git config remap.commitRefs true`
# のリポジトリで commit-refs remap（../commit-refs）に旧と新の対応を渡す。置き換えはコミット
# しないので、コミットが要ることを表示する。プロジェクトローカルの .git/hooks/post-rewrite が
# あれば先に呼ぶ。
#
# hook のファイルを nix build で作る（hook が store のパスで呼ぶ commit-refs も一緒にビルドされる）。
# 一時 HOME には ~/.claude を置かない（hook は ~/.claude に頼らない）。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/post-rewrite-remap.sh

set -euo pipefail

HOST="${1:-r995}"
ROOT=$(git rev-parse --show-toplevel)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
echo "==> post-rewrite hook をビルドする（nixosConfigurations.${HOST}）"
BUILT=$(nix build --no-link --print-out-paths --option warn-dirty false \
  "$ROOT#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/post-rewrite\".source")
cp "$BUILT" "$HOOKS/post-rewrite"
chmod +x "$HOOKS/post-rewrite"

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
git config --global core.hooksPath "$HOOKS"

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# new_repo <dir> [opt-in]: main に初期コミットを持つ repo を作る。第 2 引数があればオプトインする
new_repo() {
  git init -q -b main "$1"
  echo init > "$1/README.md"
  git -C "$1" add README.md
  git -C "$1" commit -q -m "chore: init"
  if [[ -n "${2:-}" ]]; then
    git -C "$1" config remap.commitRefs true
  fi
}

# branch_with_record <repo>: feat に測ったコミットとそれを指す記録を作り、main を 1 つ進める。
# 測ったコミットの番号を出す
branch_with_record() {
  local repo="$1" old
  git -C "$repo" switch -q -c feat
  echo measured > "$repo/src.txt"
  git -C "$repo" add src.txt
  git -C "$repo" commit -q -m "feat: measured"
  old=$(git -C "$repo" rev-parse HEAD)
  printf 'measured at %s\n' "${old:0:8}" > "$repo/notes.md"
  git -C "$repo" add notes.md
  git -C "$repo" commit -q -m "docs: record"
  git -C "$repo" switch -q main
  echo other > "$repo/other.txt"
  git -C "$repo" add other.txt
  git -C "$repo" commit -q -m "chore: main moves"
  git -C "$repo" switch -q feat
  echo "$old"
}

# --- 1. オプトインした repo の rebase -------------------------------------------------------------
REPO="$WORK/rebase"
new_repo "$REPO" opt-in
branch_with_record "$REPO" > /dev/null
if git -C "$REPO" rebase main >"$WORK/out" 2>&1; then
  new=$(git -C "$REPO" rev-parse HEAD~1)
  if [[ "$(cat "$REPO/notes.md")" == "measured at ${new:0:8}" ]]; then
    ok "オプトインした repo では rebase の後に記録の番号が新しい番号になる"
  else
    ng "rebase の後の記録が新しい番号になっていない: $(cat "$REPO/notes.md")"
  fi
  if grep -q "REFS: 1" "$WORK/out" && grep -q "コミット" "$WORK/out"; then
    ok "rebase の後に置き換えた件数とコミットが要ることを表示する"
  else
    ng "rebase の後の表示に件数かコミットの案内がない"
  fi
  if [[ "$(git -C "$REPO" status --porcelain)" == " M notes.md" ]]; then
    ok "置き換えはコミットせず、作業ツリーの変更として残す"
  else
    ng "置き換えの後の作業ツリーが想定と違う: $(git -C "$REPO" status --porcelain)"
  fi
else
  ng "オプトインした repo で rebase が失敗した"
fi

# --- 2. オプトインした repo の amend --------------------------------------------------------------
REPO="$WORK/amend"
new_repo "$REPO" opt-in
echo measured > "$REPO/src.txt"
git -C "$REPO" add src.txt
git -C "$REPO" commit -q -m "feat: measured"
old=$(git -C "$REPO" rev-parse HEAD)
printf 'result from %s\n' "${old:0:8}" > "$REPO/README.md"
echo fix >> "$REPO/src.txt"
git -C "$REPO" add src.txt
if git -C "$REPO" commit --amend --no-edit >"$WORK/out" 2>&1; then
  new=$(git -C "$REPO" rev-parse HEAD)
  if [[ "$(cat "$REPO/README.md")" == "result from ${new:0:8}" ]]; then
    ok "オプトインした repo では amend の後に記録の番号が新しい番号になる"
  else
    ng "amend の後の記録が新しい番号になっていない: $(cat "$REPO/README.md")"
  fi
else
  ng "オプトインした repo で amend が失敗した"
fi

# --- 2b. rebase の途中の amend では置き換えず、rebase の終わりに置き換える -------------------
REPO="$WORK/rebase-amend"
new_repo "$REPO" opt-in
old=$(branch_with_record "$REPO")
# 測ったコミット（feat の 1 つ目）で止め、2 回 amend してから続ける
if GIT_SEQUENCE_EDITOR="sed -i '1s/^pick/edit/'" git -C "$REPO" rebase -i main >"$WORK/out" 2>&1; then
  # 止めた所にある文書（main の README.md）が指す番号を含む amend の対応を hook に渡す。
  # 実物の amend の対応は、止めた所にまだない文書（notes.md）しか指さないので直接渡す
  base=$(git -C "$REPO" rev-parse main)
  printf 'base %s\n' "${base:0:8}" > "$REPO/README.md"
  git -C "$REPO" commit -q -a --amend --no-edit >/dev/null 2>&1
  readme=$(cat "$REPO/README.md")
  printf '%s %s\n' "$base" 1111111111111111111111111111111111111111 \
    | (cd "$REPO" && "$HOOKS/post-rewrite" amend) >"$WORK/out" 2>&1
  if [[ "$(cat "$REPO/README.md")" == "$readme" ]] && [[ -z "$(git -C "$REPO" status --porcelain)" ]]; then
    ok "rebase の途中の amend では作業ツリーを変えない"
  else
    ng "rebase の途中の amend で作業ツリーが変わった"
  fi
  echo fix2 >> "$REPO/src.txt"
  git -C "$REPO" commit -q -a --amend --no-edit >/dev/null 2>&1
  if git -C "$REPO" rebase --continue >"$WORK/out" 2>&1; then
    new=$(git -C "$REPO" rev-parse HEAD~1)
    if [[ "$(cat "$REPO/notes.md")" == "measured at ${new:0:8}" ]]; then
      ok "rebase の途中で amend しても、rebase の終わりに最後の番号へ置き換える"
    else
      ng "rebase の途中で amend したあとの記録が最後の番号になっていない: $(cat "$REPO/notes.md")（元 ${old:0:8}）"
    fi
  else
    ng "rebase --continue が失敗した"
  fi
else
  ng "rebase -i が edit で止まらなかった"
fi

# --- 3. オプトインしていない repo -----------------------------------------------------------------
REPO="$WORK/plain"
new_repo "$REPO"
old=$(branch_with_record "$REPO")
if git -C "$REPO" rebase main >"$WORK/out" 2>&1; then
  if [[ "$(cat "$REPO/notes.md")" == "measured at ${old:0:8}" ]] \
    && [[ -z "$(git -C "$REPO" status --porcelain)" ]]; then
    ok "オプトインしていない repo では何も変えない"
  else
    ng "オプトインしていない repo で記録が変わった: $(cat "$REPO/notes.md")"
  fi
else
  ng "オプトインしていない repo で rebase が失敗した"
fi

# --- 4. プロジェクトローカルの post-rewrite を先に呼ぶ --------------------------------------------
REPO="$WORK/local"
new_repo "$REPO" opt-in
cat > "$REPO/.git/hooks/post-rewrite" <<'HOOK'
#!/usr/bin/env bash
# 呼ばれた引数と入力、その時点の記録を残す（グローバルの置き換えより先に呼ばれたかを見る）
{ echo "args: $*"; cat; echo "notes: $(cat notes.md)"; } > "$(git rev-parse --git-dir)/local-hook.log"
HOOK
chmod +x "$REPO/.git/hooks/post-rewrite"
old=$(branch_with_record "$REPO")
if git -C "$REPO" rebase main >"$WORK/out" 2>&1; then
  LOG="$REPO/.git/local-hook.log"
  if [[ -f "$LOG" ]] && grep -q "^args: rebase$" "$LOG" && grep -q "^$old " "$LOG"; then
    ok "プロジェクトローカルの post-rewrite に引数と旧新の対応を渡す"
  else
    ng "プロジェクトローカルの post-rewrite が呼ばれていないか、入力が違う"
  fi
  if [[ -f "$LOG" ]] && grep -q "^notes: measured at ${old:0:8}$" "$LOG"; then
    ok "プロジェクトローカルの post-rewrite を置き換えより先に呼ぶ"
  else
    ng "プロジェクトローカルの post-rewrite が置き換えの後に呼ばれた"
  fi
  new=$(git -C "$REPO" rev-parse HEAD~1)
  if [[ "$(cat "$REPO/notes.md")" == "measured at ${new:0:8}" ]]; then
    ok "プロジェクトローカルの post-rewrite があっても置き換える"
  else
    ng "プロジェクトローカルの post-rewrite があると置き換わらない"
  fi
else
  ng "プロジェクトローカルの post-rewrite がある repo で rebase が失敗した"
fi

# --- 4b. linked worktree でも、共通の .git/hooks の post-rewrite を呼ぶ ---------------------------
REPO="$WORK/wt-main"
new_repo "$REPO" opt-in
cat > "$REPO/.git/hooks/post-rewrite" <<'HOOK'
#!/usr/bin/env bash
cat > /dev/null
echo called > "$(git rev-parse --git-common-dir)/local-hook-called"
HOOK
chmod +x "$REPO/.git/hooks/post-rewrite"
git -C "$REPO" worktree add -q -b wt "$WORK/wt-linked" main
echo x > "$WORK/wt-linked/x.txt"
git -C "$WORK/wt-linked" add x.txt
git -C "$WORK/wt-linked" commit -q -m "feat: x"
if git -C "$WORK/wt-linked" commit -q --amend -m "feat: x2" >"$WORK/out" 2>&1 \
  && [[ -f "$REPO/.git/local-hook-called" ]]; then
  ok "linked worktree でも、共通の .git/hooks の post-rewrite を呼ぶ"
else
  ng "linked worktree で共通の .git/hooks の post-rewrite が呼ばれない"
fi

# --- 5. 曖昧なら置き換えず、エラーを表示する ------------------------------------------------------
# 案内のコマンドを貼って使えるかを見るため、パスに空白を含める
REPO="$WORK/ambiguous repo"
new_repo "$REPO" opt-in
old=$(branch_with_record "$REPO")
printf 'short %s\n' "${old:0:7}" >> "$REPO/notes.md"
git -C "$REPO" commit -q -a -m "docs: short"
# 記録の 7 桁の番号が 2 つの旧に当たる入力（先頭 7 桁を共有する旧は実物の rebase ではまず
# 起きないので、hook に直接渡す）
other="${old:0:7}$(printf 'f%.0s' {1..33})"
printf '%s %s\n%s %s\n' \
  "$old" 1111111111111111111111111111111111111111 \
  "$other" 2222222222222222222222222222222222222222 \
  > "$WORK/ambiguous-input"
before=$(cat "$REPO/notes.md")
if (cd "$REPO" && "$HOOKS/post-rewrite" rebase < "$WORK/ambiguous-input") >"$WORK/out" 2>&1; then
  ng "曖昧な対応で hook が成功を返した"
else
  if [[ "$(cat "$REPO/notes.md")" == "$before" ]] && grep -q "ERROR:" "$WORK/out"; then
    ok "曖昧な対応では何も変えず、エラーを表示する"
  else
    ng "曖昧な対応で記録が変わったか、エラーが表示されない"
  fi
  # hook は正規化した絶対パスを出す（TMPDIR がシンボリックリンクの下でも比べられるよう、同じ形で求める）
  saved="$(git -C "$REPO" rev-parse --absolute-git-dir)/commit-refs-remap.input"
  if grep -qF "直し方: 上の ERROR を直してから、commit-refs remap $(printf '%q' "$saved")" "$WORK/out" \
    && cmp -s "$WORK/ambiguous-input" "$saved"; then
    ok "失敗したときは対応を残し、commit-refs remap で呼び直す方法を表示する"
  else
    ng "失敗したときに対応が残らないか、呼び直す方法が表示されない"
  fi
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
