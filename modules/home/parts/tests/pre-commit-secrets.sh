#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook が、Claude Code のセッションのコミットで、ステージした中身の機密情報を gitleaks で
# 止めることを検証する（ADR-0015）
# =============================================================================
# コミットの時点の index を見るので、同じコマンドの git add と commit -a で足した変更も検査する。漏えいは場所と
# ルール ID だけを出し、値は出さない。ALLOW_SECRET_COMMIT=<理由> で迂回でき、Claude Code のセッションでなければ
# 検査しない。プロジェクトのローカルの pre-commit があっても、fork でも検査する。
#
# hook のファイルを nix build で作る（hook が store のパスで呼ぶ gitleaks も一緒にビルドされる）。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-secrets.sh

set -euo pipefail

HOST="${1:-r995}"
ROOT=$(git rev-parse --show-toplevel)

if ! command -v markdownlint >/dev/null 2>&1; then
  echo "✗ markdownlint が PATH にない。自動修正がステージし直した後の検査を確かめられないので、このテストは成立しない"
  exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
for hook in pre-commit pre-merge-commit; do
  echo "==> $hook hook をビルドする（nixosConfigurations.${HOST}）"
  cp "$(nix build --no-link --print-out-paths --option warn-dirty false \
    "$ROOT#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/$hook\".source")" \
    "$HOOKS/$hook"
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
unset CLAUDECODE ALLOW_SECRET_COMMIT

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# gitleaks の aws-access-token に当たる鍵。このファイル自体が検査に当たらないよう、実行時につなぐ
KEY="AKIA""Z3QK7VYH2PLX4N6M"

# new_repo <dir>: feature ブランチに初期コミットを持つ repo を作る（main ガードを避ける）
new_repo() {
  git init -q -b main "$1"
  echo init > "$1/init.txt"
  git -C "$1" add init.txt
  git -C "$1" commit -q -m "chore: init"
  git -C "$1" switch -q -c feat/x
}

# --- 1. 同じコマンドで add した鍵を止め、場所とルール ID を出し、値は出さない -----------------------------
REPO="$WORK/leak"
new_repo "$REPO"
printf 'aws_access_key_id = %s\n' "$KEY" > "$REPO/conf.txt"
if (cd "$REPO" && git add conf.txt && CLAUDECODE=1 git commit -q -m "chore: conf") >"$WORK/out" 2>&1; then
  ng "同じコマンドで add した鍵で止まらない"
else
  if grep -q "conf.txt:1" "$WORK/out" && grep -q "aws-access-token" "$WORK/out" && ! grep -qF "$KEY" "$WORK/out"; then
    ok "同じコマンドで add した鍵で止め、場所とルール ID を出し、値は出さない"
  else
    ng "鍵で止めたが、場所とルール ID の出し方か、値の隠し方が違う"
  fi
fi

# --- 2. 鍵の無いコミットは通す ------------------------------------------------------------------------
REPO="$WORK/clean"
new_repo "$REPO"
echo plain > "$REPO/a.txt"
git -C "$REPO" add a.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: a" >"$WORK/out" 2>&1; then
  ok "鍵の無いコミットは通す"
else
  ng "鍵の無いコミットで止めた"
fi

# --- 3. commit -a で足した変更も見る ------------------------------------------------------------------
printf 'key = %s\n' "$KEY" > "$REPO/a.txt"
if CLAUDECODE=1 git -C "$REPO" commit -q -am "chore: a2" >"$WORK/out" 2>&1; then
  ng "commit -a で足した鍵で止まらない"
elif grep -q "a.txt:1" "$WORK/out" && grep -q "aws-access-token" "$WORK/out"; then
  ok "commit -a で足した鍵で止める"
else
  ng "commit -a で止めたが、鍵の場所とルール ID を出さない"
fi
git -C "$REPO" checkout -q -- a.txt

# --- 4. 迂回と範囲 -------------------------------------------------------------------------------------
REPO="$WORK/escape"
new_repo "$REPO"
printf 'key = %s\n' "$KEY" > "$REPO/b.txt"
git -C "$REPO" add b.txt
# 値は理由の文字列（ADR-0023。claude-hooks と同じ規則: 空白を除き ASCII 1・それ以外 2 で幅 6 以上）
if ALLOW_SECRET_COMMIT=誤検出のため CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b" >"$WORK/out" 2>&1; then
  ok "ALLOW_SECRET_COMMIT=<理由> で迂回できる"
else
  ng "ALLOW_SECRET_COMMIT=<理由> でも止めた"
fi
printf 'key = %s\n' "$KEY" > "$REPO/b2.txt"
git -C "$REPO" add b2.txt
if ALLOW_SECRET_COMMIT=1 CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b2" >"$WORK/out" 2>&1; then
  ng "ALLOW_SECRET_COMMIT=1 で迂回できた（理由の無い値）"
elif grep -q "ALLOW_SECRET_COMMIT には理由を記載" "$WORK/out" && ! grep -q "6" "$WORK/out"; then
  ok "ALLOW_SECRET_COMMIT=1 は理由を求めて止め、しきい値を明かさない"
else
  ng "ALLOW_SECRET_COMMIT=1 を止めたが、理由を求める文言でない"
fi
if ALLOW_SECRET_COMMIT=force CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b2" >"$WORK/out" 2>&1; then
  ng "ALLOW_SECRET_COMMIT=force で迂回できた（幅 5）"
else
  ok "ALLOW_SECRET_COMMIT=force は止める（幅 5）"
fi
if ALLOW_SECRET_COMMIT="a　　　b" CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b2" >"$WORK/out" 2>&1; then
  ng "全角空白で幅を水増しした値で迂回できた"
else
  ok "全角空白で幅を水増しした値は止める"
fi
# バックスラッシュは幅に数えない（claude-hooks と同じ）
if ALLOW_SECRET_COMMIT='\\\\\\' CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b2" >"$WORK/out" 2>&1; then
  ng "バックスラッシュで幅を水増しした値で迂回できた"
else
  ok "バックスラッシュで幅を水増しした値は止める"
fi
# EM SPACE（U+2003）。claude-hooks の char::is_whitespace と同じ範囲を除く
if ALLOW_SECRET_COMMIT="$(printf 'a\342\200\203\342\200\203\342\200\203\342\200\203b')" CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b2" >"$WORK/out" 2>&1; then
  ng "EM SPACE で幅を水増しした値で迂回できた"
else
  ok "EM SPACE で幅を水増しした値は止める"
fi
if ALLOW_SECRET_COMMIT=abcdef CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: b2" >"$WORK/out" 2>&1; then
  ok "ALLOW_SECRET_COMMIT=abcdef で迂回できる（幅 6）"
else
  ng "ALLOW_SECRET_COMMIT=abcdef でも止めた（幅 6）"
fi
if git -C "$REPO" diff --cached --quiet; then :; else git -C "$REPO" restore --staged b2.txt; rm -f "$REPO/b2.txt"; fi
printf 'key = %s\n' "$KEY" > "$REPO/c.txt"
git -C "$REPO" add c.txt
if git -C "$REPO" commit -q -m "chore: c" >"$WORK/out" 2>&1; then
  ok "Claude Code のセッションでなければ検査しない"
else
  ng "Claude Code のセッションでないのに止めた"
fi

# --- 5. ローカルの pre-commit があっても、fork でも検査する ----------------------------------------------
REPO="$WORK/local"
new_repo "$REPO"
printf '#!/bin/sh\nexit 0\n' > "$REPO/.git/hooks/pre-commit"
chmod +x "$REPO/.git/hooks/pre-commit"
git -C "$REPO" remote add upstream https://example.com/upstream.git
printf 'key = %s\n' "$KEY" > "$REPO/d.txt"
git -C "$REPO" add d.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: d" >"$WORK/out" 2>&1; then
  ng "ローカルの pre-commit がある fork で、鍵で止まらない"
elif grep -q "d.txt:1" "$WORK/out" && grep -q "aws-access-token" "$WORK/out"; then
  ok "ローカルの pre-commit がある fork でも、鍵で止める"
else
  ng "ローカルの pre-commit がある fork で止めたが、鍵の場所とルール ID を出さない"
fi

# --- 6. Markdown の自動修正がステージし直した中身も見る ----------------------------------------------------
# 自動修正は作業ツリーをステージし直す（#282）。ステージしていない部分の鍵も、コミットに入るので止める
REPO="$WORK/restage"
new_repo "$REPO"
printf '# t\n\nplain\n' > "$REPO/notes.md"
git -C "$REPO" add notes.md
printf '# t\n\nplain\n\nkey = %s\n' "$KEY" > "$REPO/notes.md"
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: notes" >"$WORK/out" 2>&1; then
  ng "Markdown の自動修正がステージし直した鍵を、検査せずにコミットした"
elif grep -q "notes.md:5" "$WORK/out" && grep -q "aws-access-token" "$WORK/out"; then
  ok "Markdown の自動修正がステージし直した鍵でも止める"
else
  ng "Markdown の自動修正の後で止めたが、鍵の場所とルール ID を出さない"
fi

# --- 6b. 種類の変更（シンボリックリンクから普通のファイル）だけのコミットも見る --------------------------
REPO="$WORK/typechange"
new_repo "$REPO"
ln -s init.txt "$REPO/config"
git -C "$REPO" add config
git -C "$REPO" commit -q -m "chore: link"
rm "$REPO/config"
printf 'key = %s\n' "$KEY" > "$REPO/config"
git -C "$REPO" add config
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: config" >"$WORK/out" 2>&1; then
  ng "種類の変更だけのコミットで、鍵で止まらない"
elif grep -q "config:1" "$WORK/out"; then
  ok "種類の変更だけのコミットでも、鍵で止める"
else
  ng "種類の変更だけのコミットで止めたが、鍵の場所を出さない"
fi

# --- 6c. ローカルの pre-commit がステージし直した中身も見る ---------------------------------------------
REPO="$WORK/localrestage"
new_repo "$REPO"
printf '#!/bin/sh\ngit add f.txt\n' > "$REPO/.git/hooks/pre-commit"
chmod +x "$REPO/.git/hooks/pre-commit"
echo plain > "$REPO/f.txt"
git -C "$REPO" add f.txt
printf 'key = %s\n' "$KEY" > "$REPO/f.txt"
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: f" >"$WORK/out" 2>&1; then
  ng "ローカルの pre-commit がステージし直した鍵を、検査せずにコミットした"
elif grep -q "f.txt:1" "$WORK/out"; then
  ok "ローカルの pre-commit がステージし直した鍵でも止める"
else
  ng "ローカルの pre-commit の後で止めたが、鍵の場所を出さない"
fi

# --- 6d. ローカルの pre-commit の無い fork でも見る ------------------------------------------------------
REPO="$WORK/fork"
new_repo "$REPO"
git -C "$REPO" remote add upstream https://example.com/upstream.git
printf 'key = %s\n' "$KEY" > "$REPO/g.txt"
git -C "$REPO" add g.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: g" >"$WORK/out" 2>&1; then
  ng "fork で、鍵で止まらない"
elif grep -q "g.txt:1" "$WORK/out"; then
  ok "fork でも、鍵で止める"
else
  ng "fork で止めたが、鍵の場所を出さない"
fi

# --- 6e. 競合の無いマージ（pre-merge-commit だけが走る）でも、相手のブランチから入る中身を見る -------------
REPO="$WORK/merge"
new_repo "$REPO"
git -C "$REPO" switch -q -c feat/k
printf 'key = %s\n' "$KEY" > "$REPO/k.txt"
git -C "$REPO" add k.txt
git -C "$REPO" commit -q -m "chore: k"
git -C "$REPO" switch -q feat/x
if CLAUDECODE=1 git -C "$REPO" merge -q --no-ff -m "Merge feat/k" feat/k >"$WORK/out" 2>&1; then
  ng "競合の無いマージで、相手のブランチの鍵で止まらない"
elif grep -q "k.txt:1" "$WORK/out"; then
  ok "競合の無いマージでも、相手のブランチの鍵で止める"
else
  ng "競合の無いマージで止めたが、鍵の場所を出さない"
fi

# --- 6b. 短い値のエスケープは、gitleaks が失敗したときも理由を求める行を足す（claude-hooks と同じ）---------
REPO="$WORK/broken-short"
new_repo "$REPO"
printf 'this is not toml [\n' > "$REPO/.gitleaks.toml"
echo plain > "$REPO/e.txt"
git -C "$REPO" add e.txt
if ALLOW_SECRET_COMMIT=1 CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: e" >"$WORK/out" 2>&1; then
  ng "短い値のエスケープで、gitleaks が失敗したのにコミットを通した"
elif grep -q "gitleaks が失敗しました" "$WORK/out" && grep -q "ALLOW_SECRET_COMMIT には理由を記載" "$WORK/out"; then
  ok "短い値のエスケープは、gitleaks が失敗したときも理由を求める行を足す"
else
  ng "短い値のエスケープで gitleaks が失敗したが、失敗と理由の両方を出さない"
fi

# --- 7. gitleaks が失敗したら、漏えいと分けて理由を出して止める ------------------------------------------
REPO="$WORK/broken"
new_repo "$REPO"
printf 'this is not toml [\n' > "$REPO/.gitleaks.toml"
echo plain > "$REPO/e.txt"
git -C "$REPO" add e.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "chore: e" >"$WORK/out" 2>&1; then
  ng "gitleaks が失敗したのにコミットを通した"
elif grep -q "gitleaks が失敗しました" "$WORK/out" && ! grep -q "機密情報らしき内容" "$WORK/out" \
  && ! grep -q "integer expression" "$WORK/out"; then
  ok "gitleaks が失敗したら、漏えいと分けて理由を出して止める"
else
  ng "gitleaks の失敗を、漏えいとして出した"
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
