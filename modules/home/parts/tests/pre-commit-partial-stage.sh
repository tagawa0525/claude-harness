#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook の既定の検査が、一部だけステージしたファイルで、ステージした中身を検査し、
# ステージしていない編集をコミットに入れず、作業ツリーに残すことを検証する
# =============================================================================
# ../git.nix の pre-commit は core.hooksPath でグローバル配布される。既定の検査（nixfmt、ruff、rustfmt、
# markdownlint の自動修正）は、ステージした版を書き出したものか標準入力で見て、作業ツリーを書き換えない。
# Markdown の自動修正はステージした版にだけ当て、作業ツリーは、ステージした版と同じときだけ直す。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-partial-stage.sh

set -euo pipefail

HOST="${1:-r995}"

for tool in nixfmt ruff markdownlint rustfmt cargo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "✗ $tool が PATH にない。既定の検査が走ったかを確かめられないので、このテストは成立しない"
    exit 1
  fi
done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> pre-commit hook をビルドする（nixosConfigurations.${HOST}）"
cp "$(nix build --no-link --print-out-paths --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".source")" \
  "$WORK/pre-commit"
chmod +x "$WORK/pre-commit"

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# repo <name> <file> <初めの中身>: <file> を 1 つコミットした repo を作り、そのパスを出す
repo() {
  local dir="$WORK/$1"
  git init -q -b main "$dir"
  printf '%s' "$3" >"$dir/$2"
  git -C "$dir" add "$2"
  git -C "$dir" commit -q --no-verify -m "chore: init"
  echo "$dir"
}

# stage <dir> <file> <ステージする中身> <作業ツリーに残す中身>
stage() {
  printf '%s' "$3" >"$1/$2"
  git -C "$1" add "$2"
  printf '%s' "$4" >"$1/$2"
}

# commit <dir>: hook を走らせて（git commit と同じく作業ツリーの根で）結果を返す
commit() {
  (cd "$1" && "$WORK/pre-commit") >"$WORK/out" 2>&1
}

LINES=$'# T\n\nl1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\n'

# unchanged <file> <前の中身> <前の mtime>: 作業ツリーのファイルが、中身も mtime も変わっていない
unchanged() {
  [[ "$(cat "$1")" == "${2%$'\n'}" && "$(stat -c %.9Y "$1")" == "$3" ]]
}

echo "==> Markdown の自動修正は、ステージした版だけを直してステージし直し、作業ツリーに触れない"
R=$(repo md a.md "$LINES")
STAGED=${LINES/l1/l1 }
WORKTREE=${STAGED/l2/l2 edited}
stage "$R" a.md "$STAGED" "$WORKTREE"
MTIME=$(stat -c %.9Y "$R/a.md")
if commit "$R"; then ok "自動修正できる Markdown のコミットを通す"; else ng "自動修正できる Markdown のコミットを通す"; fi
if [[ "$(git -C "$R" show :a.md)" == "${LINES%$'\n'}" ]]; then
  ok "ステージした版は直り、ステージしていない編集は入らない"
else
  ng "ステージした版は直り、ステージしていない編集は入らない"
  git -C "$R" show :a.md | sed 's/^/    index: /'
fi
if unchanged "$R/a.md" "$WORKTREE" "$MTIME"; then
  ok "一部だけステージしたファイルの作業ツリーは書き換えない"
else
  ng "一部だけステージしたファイルの作業ツリーは書き換えない"
  sed 's/^/    worktree: /' "$R/a.md"
fi

echo "==> Nix と Python は、ステージした中身を検査する"
GOOD_NIX=$'{ a = 1; }\n'
BAD_NIX=$'{a=1;}\n'
R=$(repo nix-bad a.nix "$GOOD_NIX")
stage "$R" a.nix "$BAD_NIX" "$GOOD_NIX"
MTIME=$(stat -c %.9Y "$R/a.nix")
if commit "$R"; then ng "ステージした Nix が崩れていれば止める"; else ok "ステージした Nix が崩れていれば止める"; fi
if unchanged "$R/a.nix" "$GOOD_NIX" "$MTIME"; then
  ok "止めても、作業ツリーは書き換えない"
else
  ng "止めても、作業ツリーは書き換えない"
fi
R=$(repo nix-good a.nix "$GOOD_NIX")
stage "$R" a.nix $'{ a = 2; }\n' "$BAD_NIX"
if commit "$R"; then ok "ステージした Nix が整っていれば、作業ツリーが崩れていても通す"; else ng "ステージした Nix が整っていれば、作業ツリーが崩れていても通す"; fi

R=$(repo py a.py $'x = 1\n')
stage "$R" a.py $'import os\n\nx = 1\n' $'x = 2\n'
if commit "$R"; then ng "ステージした Python に違反があれば止める"; else ok "ステージした Python に違反があれば止める"; fi

R=$(repo py-good a.py $'x = 1\n')
stage "$R" a.py $'x = 2\n' $'import os\n\nx = 2\n'
if commit "$R"; then ok "ステージした Python が整っていれば、作業ツリーに違反があっても通す"; else ng "ステージした Python が整っていれば、作業ツリーに違反があっても通す"; fi

echo "==> Rust は、ステージした版をクレートの edition で検査する"
R=$(repo rs Cargo.toml $'[package]\nname = "x"\nversion = "0.1.0"\nedition = "2021"\n')
mkdir -p "$R/src"
GOOD_RS=$'fn main() {}\n'
BAD_RS=$'fn main(){}\n'
stage "$R" src/main.rs "$BAD_RS" "$GOOD_RS"
if commit "$R"; then ng "ステージした Rust が崩れていれば止める"; else ok "ステージした Rust が崩れていれば止める"; fi
git -C "$R" rm -q --cached -f src/main.rs
stage "$R" src/main.rs "$GOOD_RS" "$BAD_RS"
if commit "$R"; then ok "ステージした Rust が整っていれば、作業ツリーが崩れていても通す"; else ng "ステージした Rust が整っていれば、作業ツリーが崩れていても通す"; fi

echo "==> 差分の設定や index の状態によらず、作業ツリーに触れない"
R=$(repo settings a.md "$LINES")
git -C "$R" config diff.noprefix true
printf 'x.dat diff=upper\n' >"$R/.gitattributes"
git -C "$R" config diff.upper.textconv 'tr a-z A-Z <'
printf 'aa\n' >"$R/x.dat"
git -C "$R" add .gitattributes x.dat
git -C "$R" commit -q --no-verify -m "chore: data"
printf 'aa\ncc\n' >"$R/x.dat"
printf 'hello\n' >"$R/new.txt"
git -C "$R" add -N new.txt
STAGED=${LINES/l1/l1 }
WORKTREE=${STAGED/l2/l2 edited}
stage "$R" a.md "$STAGED" "$WORKTREE"
if commit "$R"; then ok "diff.noprefix、textconv、git add -N があっても通す"; else ng "diff.noprefix、textconv、git add -N があっても通す"; fi
if [[ "$(cat "$R/x.dat")" == $'aa\ncc' && "$(cat "$R/new.txt")" == hello && "$(cat "$R/a.md")" == "${WORKTREE%$'\n'}" ]] &&
  git -C "$R" diff --name-only | grep -qx new.txt; then
  ok "ステージしていない編集と git add -N のファイルが、そのまま残る"
else
  ng "ステージしていない編集と git add -N のファイルが、そのまま残る"
fi

echo "==> 改行の変換があっても、全てステージした Markdown の作業ツリーを直す"
R=$(repo crlf a.md "$LINES")
git -C "$R" config core.autocrlf true
printf '%s' "${LINES/l1/l1 }" | sed 's/$/\r/' >"$R/a.md"
git -C "$R" add a.md 2>/dev/null
if commit "$R" && [[ "$(git -C "$R" show :a.md)" == "${LINES%$'\n'}" ]] && git -C "$R" diff --quiet &&
  grep -q $'l1\r$' "$R/a.md"; then
  ok "作業ツリーを、改行の変換を保って直す"
else
  ng "作業ツリーを、改行の変換を保って直す"
fi

echo "==> 作業ツリーの文字の符号化が違っても、直した版をそのままステージする"
R=$(repo encoding init.txt $'x\n')
printf '*.md working-tree-encoding=UTF-16LE eol=lf\n' >"$R/.gitattributes"
git -C "$R" add .gitattributes
git -C "$R" commit -q --no-verify -m "chore: attributes"
printf '%s' "${LINES/l1/l1 }" | iconv -f UTF-8 -t UTF-16LE >"$R/a.md"
git -C "$R" add a.md
if commit "$R" && [[ "$(git -C "$R" show :a.md)" == "${LINES%$'\n'}" ]]; then
  ok "working-tree-encoding のある Markdown も、直した版をステージする"
else
  ng "working-tree-encoding のある Markdown も、直した版をステージする"
  git -C "$R" show :a.md | head -3 | sed 's/^/    index: /'
fi

echo "==> 作業ツリーのパスがシンボリックリンクなら、その先を書き換えない"
R=$(repo symlink a.md "$LINES")
STAGED=${LINES/l1/l1 }
printf '%s' "$STAGED" >"$R/a.md"
git -C "$R" add a.md
printf '%s' "$STAGED" >"$WORK/outside.md"
rm "$R/a.md"
ln -s "$WORK/outside.md" "$R/a.md"
if commit "$R" && [[ "$(cat "$WORK/outside.md")" == "${STAGED%$'\n'}" ]] && [[ -L "$R/a.md" ]]; then
  ok "シンボリックリンクの先のファイルを書き換えない"
else
  ng "シンボリックリンクの先のファイルを書き換えない"
fi

echo "==> clean のフィルタが落とす作業ツリーだけの行も、書き戻しで消さない"
R=$(repo lossy init.txt $'x\n')
printf '*.md filter=drop\n' >"$R/.gitattributes"
git -C "$R" config filter.drop.clean "sed '/^LOCAL-ONLY/d'"
git -C "$R" add .gitattributes
git -C "$R" commit -q --no-verify -m "chore: attributes"
printf '%sLOCAL-ONLY note\n' "${LINES/l1/l1 }" >"$R/a.md"
git -C "$R" add a.md
if commit "$R" && grep -qx 'LOCAL-ONLY note' "$R/a.md"; then
  ok "作業ツリーにしかない行を残す"
else
  ng "作業ツリーにしかない行を残す"
fi

echo "==> - で始まるパスも検査する"
R=$(repo dash init.txt $'x\n')
mkdir -p "$R/-docs"
printf '%s' "${LINES/l1/l1 }" >"$R/-docs/a.md"
git -C "$R" add -- -docs/a.md
if commit "$R" && [[ "$(git -C "$R" show :-docs/a.md)" == "${LINES%$'\n'}" ]]; then
  ok "- で始まるディレクトリの Markdown も直す"
else
  ng "- で始まるディレクトリの Markdown も直す"
fi

echo "==> staged-checks に言語を渡すと、その言語だけを検査する（run-checks.sh の Markdown の段）"
STAGED_CHECKS=$(grep -o '/nix/store/[^ ]*/bin/staged-checks' "$WORK/pre-commit" | head -1)
if [[ ! -x "$STAGED_CHECKS" ]]; then
  echo "✗ pre-commit の hook から staged-checks の場所を読めない: '$STAGED_CHECKS'"
  exit 1
fi
R=$(repo only nix-and-md.txt $'x\n')
printf '{a=1;}\n' >"$R/a.nix"
printf '%s' "${LINES/l1/l1 }" >"$R/a.md"
git -C "$R" add a.nix a.md
if (cd "$R" && "$STAGED_CHECKS" markdown) >"$WORK/out" 2>&1 && [[ "$(git -C "$R" show :a.md)" == "${LINES%$'\n'}" ]]; then
  ok "markdown だけを渡すと、崩れた Nix で止めず、Markdown を直す"
else
  ng "markdown だけを渡すと、崩れた Nix で止めず、Markdown を直す"
fi
if (cd "$R" && "$STAGED_CHECKS" nix) >"$WORK/out" 2>&1; then
  ng "nix を渡すと、崩れた Nix で止める"
else
  ok "nix を渡すと、崩れた Nix で止める"
fi
rc=0
(cd "$R" && "$STAGED_CHECKS" fortran) >"$WORK/out" 2>&1 || rc=$?
if [[ $rc -eq 2 ]]; then
  ok "知らない言語は使い方の誤り（終了コード 2）として止める"
else
  ng "知らない言語は使い方の誤り（終了コード 2）として止める（終了コード $rc）"
fi

echo "==> ステージしていない編集が無ければ、今までどおり"
R=$(repo plain a.md "$LINES")
printf '%s' "${LINES/l1/l1 }" >"$R/a.md"
git -C "$R" add a.md
if commit "$R" && [[ "$(git -C "$R" show :a.md)" == "${LINES%$'\n'}" ]] && git -C "$R" diff --quiet; then
  ok "全てステージした Markdown は直してステージし直す"
else
  ng "全てステージした Markdown は直してステージし直す"
fi

echo
echo "passed: $PASS, failed: $FAIL"
((FAIL == 0))
