#!/usr/bin/env bash
# claude-hooks の check-merge-body-ja-terms が、ja-terms と辞書（Linux は nix のラッパーが渡すもの、Windows の CI は
# cargo install して PATH にあるもの。ja-terms の
# prh.yml）で git merge の本文の崩れた語と、--no-ff のマージの Conventional Commits でない件名を止め、正しい語と、
# commit-msg が見ない Merge の件名を通すことを確かめる（package.nix の installCheckPhase と、Windows の CI の windows.yml が呼ぶ）。本文の取り出しは、偽の ja-terms を使う
# ユニットテスト（src/rules/check_merge_body_ja_terms.rs）が見る。
# 崩れた語をわざと含むので、語の検査から外す（.textlintignore）
#
# ja-terms は共通の辞書をホームの下（claude-setup sync が配る ~/.config/ja-terms/prh.yml）か、環境変数
# JA_TERMS_COMMON_DICTIONARY のパスから読む。ホームに辞書の無い所では、呼ぶ側がソースの辞書を渡す（ADR-0097）
#
# 使い方: merge-ja-terms.sh <claude-hooks の実行ファイル>
set -euo pipefail

BIN=$1
# 下の out=$(…) にコマンドの出力を入れる。nix の devShell やビルドは out を export するので、そのままだと出力が
# 子の環境に入り、128 KB を超えると次の exec が E2BIG で失敗する
export -n out
REPO=$(mktemp -d)
trap 'rm -rf "$REPO" "$REPO.gitconfig"' EXIT
# ルールは、グローバルの core.hooksPath（commit-msg の置き場）を使うリポジトリだけを検査する
export GIT_CONFIG_GLOBAL="$REPO.gitconfig"
git config --global core.hooksPath /nonexistent/global-hooks
git -C "$REPO" init -q

run() {
  jq -n --arg c "$1" --arg d "$REPO" '{tool_name: "Bash", tool_input: {command: $c}, cwd: $d}' |
    "$BIN" pre-tool-use --rule check-merge-body-ja-terms
}

fail() {
  echo "FAIL: $1" >&2
  echo "$2" >&2
  exit 1
}

out=$(run "git merge --no-ff feat/a -m \"\$(cat <<'EOF'
Merge: a

## Why
掃引して計算を流し直す
EOF
)\"")
decision=$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$out")
[ "$decision" = deny ] || fail "崩れた語を止めるはず" "$out"
reason=$(jq -r '.hookSpecificOutput.permissionDecisionReason' <<<"$out")
for term in "掃引 => スイープ" "流し直す => 再実行する"; do
  grep -qF "$term" <<<"$reason" || fail "「$term」を示すはず" "$out"
done

out=$(run "git merge --no-ff feat/a -m 'Merge: a' -m '確認の結果を再実行する'")
[ -z "$out" ] || fail "正しい語は何も出さずに通すはず" "$out"

out=$(run "git merge --no-ff feat/a -m 'Merge: 掃引' -m '確認'")
[ -z "$out" ] || fail "commit-msg と同じく Merge の件名は検査しないはず" "$out"

# --no-ff の無いマージは、本文の語だけを ja-terms message で見る（件名の形では止めない）
out=$(run "git merge feat/a -m '何の型もない件名' -m '掃引して計算を流し直す'")
reason=$(jq -r '.hookSpecificOutput.permissionDecisionReason // empty' <<<"$out")
grep -qxF "  掃引 => スイープ" <<<"$reason" || fail "--no-ff の無いマージの本文の語を、行と列を外して示すはず" "$out"
out=$(run "git merge feat/a -m '何の型もない件名' -m '確認'")
[ -z "$out" ] || fail "--no-ff の無いマージは件名の形では止めないはず" "$out"

# commit-msg は Merge などで始まらない件名の形も見るので、同じ検査で先に止める（#118）
out=$(run "git merge --no-ff feat/a -m '何の型もない件名'")
[ "$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$out")" = deny ] || fail "Conventional Commits でない件名を止めるはず" "$out"
grep -qF "Conventional Commits" <<<"$(jq -r '.hookSpecificOutput.permissionDecisionReason' <<<"$out")" ||
  fail "件名の形の理由を示すはず" "$out"

# リポジトリの辞書（.ja-terms/prh.yml。ADR-0096）は、コマンドの行き先のリポジトリのものを、共通の辞書に重ねて当てる。
# 行き先が hook の cwd と違う git -C の形でも、行き先のリポジトリの辞書を読む
OTHER=$(mktemp -d)
trap 'rm -rf "$REPO" "$REPO.gitconfig" "$OTHER"' EXIT
git -C "$OTHER" init -q
mkdir "$OTHER/.ja-terms"
printf 'version: 1\nrules:\n  - expected: ほげ\n    pattern: ふがふが\n' >"$OTHER/.ja-terms/prh.yml"
# コマンドの文字列の中のパスは claude-hooks が読む。Windows（Git Bash）の /tmp の形は claude-hooks.exe に通じないので、
# Windows の形にする
if command -v cygpath >/dev/null; then OTHER=$(cygpath -m "$OTHER"); fi
out=$(run "git -C '$OTHER' merge --no-ff feat/a -m 'Merge: a' -m 'ふがふがと掃引'")
reason=$(jq -r '.hookSpecificOutput.permissionDecisionReason // empty' <<<"$out")
for term in "ふがふが => ほげ" "掃引 => スイープ"; do
  grep -qF "$term" <<<"$reason" || fail "行き先のリポジトリの辞書と共通の辞書の「$term」を示すはず" "$out"
done
out=$(run "git merge --no-ff feat/a -m 'Merge: a' -m 'ふがふが'")
[ -z "$out" ] || fail "辞書の無いリポジトリには、ほかのリポジトリの辞書を当てないはず" "$out"
out=$(run "cd '$OTHER' && git merge feat/a -m 'a' -m 'ふがふが'")
grep -qxF "  ふがふが => ほげ" <<<"$(jq -r '.hookSpecificOutput.permissionDecisionReason // empty' <<<"$out")" ||
  fail "--no-ff の無いマージ（ja-terms message）も、cd の行き先のリポジトリの辞書で見るはず" "$out"

echo "ok: check-merge-body-ja-terms"
