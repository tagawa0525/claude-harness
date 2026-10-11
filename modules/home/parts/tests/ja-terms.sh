#!/usr/bin/env bash
# =============================================================================
# git の pre-commit と commit-msg hook が、崩れた日本語の語を辞書（../ja-terms/prh.yml）で止めることを
# 検証する（ADR-0014）
# =============================================================================
# pre-commit はステージした中身（作業ツリーではなく）の Markdown、テキスト、コードを、commit-msg は
# Claude Code のセッションのコミットの本文を textlint と prh で検査する。辞書の項目は、名詞の用法を
# 止め、動詞や別の語を通すことも確かめる。
#
# hook のファイルを nix build で作る（hook が store のパスで呼ぶ textlint と設定も一緒にビルドされる）。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/ja-terms.sh

set -euo pipefail

HOST="${1:-r995}"
ROOT=$(git rev-parse --show-toplevel)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
for hook in pre-commit commit-msg; do
  echo "==> $hook hook をビルドする（nixosConfigurations.${HOST}）"
  built=$(nix build --no-link --print-out-paths --option warn-dirty false \
    "$ROOT#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/$hook\".source")
  cp "$built" "$HOOKS/$hook"
  chmod +x "$HOOKS/$hook"
done

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
git config --global core.hooksPath "$HOOKS"
# GitHub リモートが無いので main ガードは効かない。commit-msg の検査は Claude Code のセッションだけ
unset CLAUDECODE

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# new_repo <dir>: main に初期コミットを持つ repo を作る
new_repo() {
  git init -q -b main "$1"
  echo init > "$1/README.md"
  git -C "$1" add README.md
  git -C "$1" commit -q --no-verify -m "chore: init"
}

# commits <repo> <path> <text>: ファイルに text を書いてステージし、コミットできれば 0
commits() {
  printf '%s\n' "$3" > "$1/$2"
  git -C "$1" add -- "$2"
  git -C "$1" commit -q -m "docs: add" >"$WORK/out" 2>&1
}

# --- 1. 辞書の項目: 名詞の用法を止め、動詞や別の語を通す ---------------------------------------------
# 本文の例は Markdown の文書としても正しい形にする（markdownlint に落ちないよう、見出しの後に 1 段落）
while IFS='|' read -r expect text; do
  [ -n "$expect" ] || continue
  REPO="$WORK/dict-$PASS-$FAIL"
  new_repo "$REPO"
  if commits "$REPO" doc.md "$(printf '# t\n\n%s\n' "$text")"; then
    result=pass
  else
    result=stop
  fi
  if [ "$result" = "$expect" ]; then
    ok "辞書: 「$text」を $expect"
  else
    ng "辞書: 「$text」を $expect のはずが $result"
  fi
done <<'CASES'
stop|確かめの結果を書く。
stop|確かめが通らない。
pass|結果を確かめる。
pass|結果を確かめた。
pass|観測者にも確かめにくい。
pass|結果は確かめがたい。
pass|結果を確かめがちだ。
pass|散歩がてら確かめがてら書く。
pass|2 つは比べがたい。
pass|結果を確かめもせずに書く。
pass|結果を確かめはしたが違う。
pass|結果を確かめ Slurm に投げる。
pass|結果を確かめに行く。
stop|比べ合わせの結果。
pass|2 つを比べ合わせると近い。
stop|2 つの比べでは差が無い。
pass|2 つを比べると差が無い。
pass|設定の効きを確かめる。
pass|目が効きます。
stop|件の数えが 3 件。
pass|二つを、比べはしたが違う。
stop|端のケースを足す。
stop|作業木を作る。
stop|掃引の範囲。
stop|点断面積を読む。
CASES

# --- 2. コードのコメントも検査する --------------------------------------------------------------------
REPO="$WORK/code"
new_repo "$REPO"
if commits "$REPO" lib.rs '// 端のケースを扱う'; then
  ng "Rust のコメントの崩れた語で止まらない"
else
  if grep -q "エッジケース" "$WORK/out"; then
    ok "Rust のコメントの崩れた語で止め、直す語を示す"
  else
    ng "Rust のコメントで止めたが、直す語を示さない"
  fi
fi

# --- 3. ステージした中身を見る（作業ツリーの、ステージしていない編集は見ない）--------------------------
# Markdown の自動修正（pre-commit-partial-stage.sh が確かめる）と切り分けるため、テキストのファイルで確かめる
REPO="$WORK/staged"
new_repo "$REPO"
printf '結果を確かめる。\n' > "$REPO/doc.txt"
git -C "$REPO" add doc.txt
printf '確かめの結果。\n' > "$REPO/doc.txt"
if git -C "$REPO" commit -q -m "docs: add" >"$WORK/out" 2>&1; then
  ok "ステージしていない編集の崩れた語では止めない"
else
  ng "ステージしていない編集の崩れた語で止めた"
fi
REPO="$WORK/staged2"
new_repo "$REPO"
printf '確かめの結果。\n' > "$REPO/doc.txt"
git -C "$REPO" add doc.txt
printf '結果を確かめる。\n' > "$REPO/doc.txt"
if git -C "$REPO" commit -q -m "docs: add" >"$WORK/out" 2>&1; then
  ng "ステージした中身の崩れた語で止めない（作業ツリーを見ている）"
else
  ok "ステージした中身の崩れた語で止める"
fi

# --- 3b. 行を折り返した動詞は止めない ----------------------------------------------------------------
REPO="$WORK/wrapped"
new_repo "$REPO"
if commits "$REPO" doc.md "$(printf '# t\n\n計算の結果を確かめ\nる。\n')"; then
  ok "行を折り返した動詞は止めない"
else
  ng "行を折り返した動詞で止めた"
fi

# --- 3c. グロブの文字を含む名前、大文字の拡張子、シンボリックリンク -------------------------------------
REPO="$WORK/names"
new_repo "$REPO"
if commits "$REPO" "[draft].md" "$(printf '# t\n\n結果を確かめる。\n')"; then
  ok "グロブの文字を含む名前の、崩れていないファイルは通す"
else
  ng "グロブの文字を含む名前の、崩れていないファイルで止めた"
fi
if commits "$REPO" "[draft2].md" "$(printf '# t\n\n確かめの結果。\n')"; then
  ng "グロブの文字を含む名前の崩れた語で止まらない"
else
  if grep -qF "[draft2].md" "$WORK/out"; then
    ok "グロブの文字を含む名前の崩れた語で止め、元のパスを示す"
  else
    ng "グロブの文字を含む名前で止めたが、元のパスを示さない"
  fi
fi
# 止めた [draft2].md はステージに残るので、外してから確かめる
git -C "$REPO" reset -q
rm -f "$REPO/[draft2].md"
if commits "$REPO" "UPPER.MD" "$(printf '# t\n\n確かめの結果。\n')"; then
  ng "大文字の拡張子の崩れた語で止まらない"
else
  ok "大文字の拡張子の崩れた語で止める"
fi
# 行き先は崩れた語を含むが、検査を通さずにコミットしておく。リンクだけをコミットして、行き先を読まないことを見る
printf '確かめの結果。\n' > "$REPO/target.txt"
git -C "$REPO" add target.txt
git -C "$REPO" commit -q --no-verify -m "chore: target"
ln -s target.txt "$REPO/link.txt"
git -C "$REPO" add link.txt
if git -C "$REPO" commit -q -m "docs: link" >"$WORK/out" 2>&1; then
  ok "シンボリックリンクは検査しない"
else
  ng "シンボリックリンクで止めた"
fi

# --- 3d. 行を折り返した「数え」、名前を変えたファイル -----------------------------------------------------
REPO="$WORK/renames"
new_repo "$REPO"
if commits "$REPO" doc.md "$(printf '# t\n\n行の数え\nる。\n')"; then
  ok "行を折り返した動詞の数えは止めない"
else
  ng "行を折り返した動詞の数えで止めた"
fi
printf '# t\n\nok\n' > "$REPO/a.md"
git -C "$REPO" add a.md
git -C "$REPO" commit -q --no-verify -m "chore: a"
git -C "$REPO" mv a.md b.md
printf '# t\n\n確かめの結果。\n' > "$REPO/z.md"
git -C "$REPO" add z.md
if git -C "$REPO" commit -q -m "docs: rename" >"$WORK/out" 2>&1; then
  ng "名前を変えたファイルと一緒にステージした崩れた語で止まらない"
else
  if grep -q "z.md" "$WORK/out" && grep -q "確認" "$WORK/out"; then
    ok "名前を変えたファイルと一緒でも、ほかのファイルの崩れた語で止める"
  else
    ng "名前を変えたファイルと一緒のときに、崩れた語ではない理由で止めた"
  fi
fi

# --- 3e. サブモジュール（gitlink）は検査しない -----------------------------------------------------------
# Markdown の自動修正は作業ツリーに無いファイルをステージから消すので、テキストの拡張子の名前で確かめる
REPO="$WORK/gitlink"
new_repo "$REPO"
git -C "$REPO" update-index --add --cacheinfo "160000,$(git -C "$REPO" rev-parse HEAD),sub.txt"
if git -C "$REPO" commit -q -m "chore: gitlink" >"$WORK/out" 2>&1; then
  ok "サブモジュール（gitlink）は検査しない"
else
  ng "サブモジュール（gitlink）で止めた"
fi

# --- 3f. 種類の変更（シンボリックリンクから普通のファイル）だけのコミットも見る ----------------------------
REPO="$WORK/typechange"
new_repo "$REPO"
echo target > "$REPO/target.txt"
ln -s target.txt "$REPO/note.txt"
git -C "$REPO" add target.txt note.txt
git -C "$REPO" commit -q --no-verify -m "chore: link"
rm "$REPO/note.txt"
printf '確かめの結果。\n' > "$REPO/note.txt"
git -C "$REPO" add note.txt
if git -C "$REPO" commit -q -m "docs: note" >"$WORK/out" 2>&1; then
  ng "種類の変更だけのコミットの崩れた語で止まらない"
elif grep -q "note.txt" "$WORK/out" && grep -q "確認" "$WORK/out"; then
  ok "種類の変更だけのコミットの崩れた語でも止める"
else
  ng "種類の変更だけのコミットで止めたが、崩れた語の場所を示さない"
fi

# --- 4. 空白を含むパス -------------------------------------------------------------------------------
REPO="$WORK/space path"
new_repo "$REPO"
mkdir -p "$REPO/a b"
if commits "$REPO" "a b/doc.md" "$(printf '# t\n\n確かめの結果。\n')"; then
  ng "空白を含むパスの崩れた語で止まらない"
else
  ok "空白を含むパスの崩れた語で止める"
fi

# --- 4b. prh の辞書は誤りを並べるので検査しない ------------------------------------------------------
REPO="$WORK/dictionary"
new_repo "$REPO"
mkdir -p "$REPO/ja-terms"
if commits "$REPO" ja-terms/prh.yml "$(printf 'rules:\n  - expected: 確認\n    pattern: 確かめの\n')"; then
  ok "prh の辞書のファイルは検査しない"
else
  ng "prh の辞書のファイルで止めた"
fi

# --- 4c. リポジトリの .textlintignore に挙げたファイルは検査しない ------------------------------------
REPO="$WORK/ignore"
new_repo "$REPO"
printf 'fixtures/**\n' > "$REPO/.textlintignore"
git -C "$REPO" add .textlintignore
git -C "$REPO" commit -q --no-verify -m "chore: ignore fixtures"
mkdir -p "$REPO/fixtures"
if commits "$REPO" fixtures/case.md "$(printf '# t\n\n確かめの結果。\n')"; then
  ok ".textlintignore に挙げたファイルは検査しない"
else
  ng ".textlintignore に挙げたファイルで止めた"
fi
if commits "$REPO" 'fixtures/[ab].md' "$(printf '# t\n\n確かめの結果。\n')"; then
  ok ".textlintignore に挙げた、[ を含む名前のファイルも検査しない"
else
  ng ".textlintignore に挙げた、[ を含む名前のファイルで止めた"
fi
if commits "$REPO" 'fixtures/a].md' "$(printf '# t\n\n確かめの結果。\n')"; then
  ok ".textlintignore に挙げた、] を含む名前のファイルも検査しない"
else
  ng ".textlintignore に挙げた、] を含む名前のファイルで止めた"
fi

# --- 5. fork では飛ばす ------------------------------------------------------------------------------
REPO="$WORK/fork"
new_repo "$REPO"
git -C "$REPO" remote add upstream https://example.com/upstream.git
if commits "$REPO" doc.md "$(printf '# t\n\n確かめの結果。\n')"; then
  ok "fork では日本語の語を検査しない"
else
  ng "fork で日本語の語を検査した"
fi

# --- 6. コミットの本文（Claude Code のセッション）---------------------------------------------------
REPO="$WORK/message"
new_repo "$REPO"
echo x > "$REPO/x.txt"
git -C "$REPO" add x.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: 確かめの結果を書く" >"$WORK/out" 2>&1; then
  ng "コミットの本文の崩れた語で止まらない"
else
  if grep -q "確認" "$WORK/out"; then
    ok "コミットの本文の崩れた語で止め、直す語を示す"
  else
    ng "コミットの本文で止めたが、直す語を示さない"
  fi
fi
echo z > "$REPO/z.txt"
git -C "$REPO" add z.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "squash! docs: add" -m "確かめの結果を書く" >"$WORK/out" 2>&1; then
  ng "squash! のコミットの本文の崩れた語で止まらない"
else
  ok "squash! のコミットの本文の崩れた語でも止める"
fi
echo y > "$REPO/y.txt"
git -C "$REPO" add y.txt
printf 'docs: 結果を確かめる\n\n本文\n# ------------------------ >8 ------------------------\n確かめの差分\n' > "$WORK/msg"
if CLAUDECODE=1 git -C "$REPO" commit -q -F "$WORK/msg" --cleanup=scissors >"$WORK/out" 2>&1; then
  ok "コミットの本文の scissors の行から後は検査しない"
else
  ng "コミットの本文の scissors の後で止めた"
fi
echo w > "$REPO/w.txt"
git -C "$REPO" add w.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: add" -m "#290 の確かめの結果" >"$WORK/out" 2>&1; then
  ng "-m で残る # で始まる行の崩れた語で止まらない"
else
  ok "-m で残る # で始まる行の崩れた語でも止める"
fi
echo v > "$REPO/v.txt"
git -C "$REPO" add v.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "fixup! docs: 確かめの結果を書く" >"$WORK/out" 2>&1; then
  ok "fixup! の件名（元のコミットから写したもの）は検査しない"
else
  ng "fixup! の件名で止めた"
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
