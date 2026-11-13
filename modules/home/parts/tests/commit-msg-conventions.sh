#!/usr/bin/env bash
# =============================================================================
# commit-msg hook の判定（Conventional Commits、件名の長さ、fork の対象外）を検証する
# =============================================================================
# ../git-hooks.nix の commit-msg hook に件名を通して、通すものと止めるものを見る。
# 自動コミットするスクリプトが書くメッセージとの整合（nix-rebuild.sh の `chore(flake): update`）は、
# スクリプトを持つ nixfiles の側のテストが確かめる。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/commit-msg-conventions.sh
#
# hook のファイルを nix build で作る（hook が呼ぶ claude-hooks と、それが包む ja-terms も一緒にビルドされる）。
# 判定の細部（ローカルの hook、fork、Claude Code の外）は claude-hooks の tests/git_hook_commit_msg.rs が偽の ja-terms で
# 確かめ、ここは配備した hook のファイルと本物の ja-terms で通すものと止めるものを見る。

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> commit-msg hook をビルドする（homeConfigurations.test）"
cp "$(nix build --no-link --print-out-paths --option warn-dirty false \
  ".#homeConfigurations.test.config.xdg.configFile.\"git/hooks/commit-msg\".source")" \
  "$WORK/commit-msg"
chmod +x "$WORK/commit-msg"

# hook は Claude Code セッションのコミットにだけ効く
export CLAUDECODE=1
# hook は git リポジトリ外だと何もせず exit 0 するため、一時リポジトリの中で実行する
# （このリポジトリ自身の .git/hooks へ委譲されないよう、新規 init したものを使う）
git init -q "$WORK/repo"
cd "$WORK/repo"

PASS=0
FAIL=0

# check <期待 ok|ng> <出所> <メッセージ>
check() {
  local expect="$1" origin="$2" msg="$3"
  printf '%s\n' "$msg" > "$WORK/msg"
  if "$WORK/commit-msg" "$WORK/msg" >"$WORK/out" 2>&1; then
    local actual=ok
  else
    local actual=ng
  fi
  if [[ "$actual" == "$expect" ]]; then
    PASS=$((PASS + 1))
    echo "✓ ${origin}: ${msg}"
  else
    FAIL=$((FAIL + 1))
    echo "✗ ${origin}: ${msg}" >&2
    echo "    expected ${expect}, got ${actual}" >&2
    sed 's/^/    /' "$WORK/out" >&2
  fi
}

echo "==> hook 自体の判定（回帰確認）"
check ok "sanity" "feat(scope): 追加する"
# nixfiles の nix-rebuild.sh が自動で書く件名。この形を止めると、毎日の自動更新が最後のコミットで失敗する
check ok "sanity" "chore(flake): update (testhost)"
check ng "sanity" "flake: update (testhost)"
check ng "sanity" "何の型もない件名"

echo "==> 件名の長さ（UTF-8 の文字数で 72 まで）と、形式の対象外の件名"
# claude-hooks の check-commit-message は、この hook と同じ検査（ja-terms commit-msg）を先に呼ぶ。検査を hook から
# 切り出しても判定が変わらないことを、境界と対象外の形で見る
# 文字数で数える（LANG=C だと ${#out} がバイト数になる）
export LC_ALL=C.UTF-8
pad() { local n=$1 c=$2 out=""; while (( ${#out} < n )); do out+=$c; done; printf '%s' "$out"; }
check ok "length" "feat: $(pad 66 a)"
check ng "length" "feat: $(pad 67 a)"
check ok "length" "docs: $(pad 66 あ)"
check ng "length" "docs: $(pad 67 あ)"
for subject in "Merge: $(pad 80 a)" "fixup! $(pad 80 a)" "squash! $(pad 80 a)" "Revert \"$(pad 80 a)\""; do
  check ok "exempt" "$subject"
done

echo "==> fork（upstream リモートのある clone）ではこちらの規約の検査を全部飛ばす"
# 他人のプロジェクトの fork では上流の慣習（"Fix socket transport when …" のような文。Serena の
# main には 72 文字を超える件名が普通にある）が正で、こちらの形式や長さを強いると上流向けの
# コミットが歪む
git init -q "$WORK/fork"
git -C "$WORK/fork" remote add upstream https://example.invalid/upstream.git
cd "$WORK/fork"
check ok "fork" "Add the proposed request"
if grep -q "飛ばします" "$WORK/out"; then
  PASS=$((PASS + 1)); echo "✓ fork: 飛ばした旨を出力する"
else
  FAIL=$((FAIL + 1)); echo "✗ fork: 飛ばした旨の出力がない" >&2; sed 's/^/    /' "$WORK/out" >&2
fi
check ok "fork" "Add a subject that is far too long for the seventy-two character limit of the hook"
cd "$WORK/repo"
check ng "plain" "Add the proposed request"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
