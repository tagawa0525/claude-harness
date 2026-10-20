#!/usr/bin/env bash
# language-checks/scripts（run-checks.sh / fix-markdown-lint.py） のテスト
#
# 実行: bash .claude/tests/scripts/language-checks.sh（まとめて実行: bash .claude/tests/scripts.sh）
#
# 実物の gh・ruff 等は使わず、make_fake_tool で PATH 先頭に置いた偽コマンドで
# 「スクリプトが何を呼び、出力をどう判定するか」を検証する。

source "$(dirname "$0")/../lib.sh"

LANG_SCRIPTS="$CLAUDE_DIR/skills/language-checks/scripts"
# 手元の staged-checks（home-manager が PATH に置く）を走らせず、どのホストでも同じ結果にする。Markdown の段を
# 確かめるテストは、この偽物を差し替える
make_fake_tool staged-checks '*) exit 0 ;;'
# 手元の squeue を呼ばず、排他のジョブが無い状態にする。排他のジョブ中の自己制限を確かめるテストが差し替える
make_fake_tool squeue '*) exit 0 ;;'

# ===========================================================================
# language-checks/scripts/run-checks.sh
# ===========================================================================

REPO="$TEST_ROOT/checks/py"
make_repo "$REPO"
cd "$REPO" || exit 1
touch pyproject.toml

it "run-checks: Python プロジェクトで format → lint → test を順に実行し ALL_OK を出す"
make_fake_tool ruff '"format --check ."*) exit 0 ;; "check ."*) exit 0 ;;'
make_fake_tool pytest '*) exit 0 ;;'
mkdir -p tests && touch tests/test_a.py
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq $'format --check .\ncheck .' "$(fake_log ruff)"
assert_contains "$out" "ALL_OK"

it "run-checks: フォーマット失敗で止まり、修正コマンドを示す（lint は実行しない）"
make_fake_tool ruff '"format --check ."*) echo "would reformat a.py"; exit 1 ;; "check ."*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 1 $?
assert_eq "format --check ." "$(fake_log ruff)"
assert_contains "$out" "FAILED: python format"
assert_contains "$out" "FIX: ruff format ."

it "run-checks: テストがなければ pytest を飛ばす"
rm -r tests
make_fake_tool ruff '*) exit 0 ;;'
make_fake_tool pytest '*) echo "should not run"; exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "" "$(fake_log pytest)"

it "run-checks: サブディレクトリのテストが落ちれば失敗する（unittest）"
make_fake_tool ruff '*) exit 0 ;;'
# host に pytest があっても unittest の経路を通すため、python3 から pytest が使えない状態にする
REAL_PY=$(command -v python3)
make_fake_tool python3 '"-m pytest --version"*) exit 1 ;; "-m unittest "*) exec '"$REAL_PY"' "$@" ;;'
mkdir -p pkg/sub
printf 'import unittest\n\n\nclass T(unittest.TestCase):\n    def test_x(self):\n        self.assertEqual(1, 2)\n' > pkg/test_a.py
git add pkg/test_a.py
out=$("$LANG_SCRIPTS/run-checks.sh" 2>&1)
assert_eq 1 $?
assert_contains "$out" "FAILED: python test: cd pkg && python3 -m unittest test_a"
assert_not_contains "$out" "ALL_OK"

it "run-checks: サブディレクトリのテストが通れば ALL_OK（unittest）"
printf 'import unittest\n\n\nclass T(unittest.TestCase):\n    def test_x(self):\n        self.assertEqual(1, 1)\n' > pkg/test_a.py
rm -rf pkg/__pycache__
out=$("$LANG_SCRIPTS/run-checks.sh" 2>&1)
assert_eq 0 $?
assert_contains "$out" "OK: python test"
remove_fake_tool python3

it "run-checks: python3 から pytest が使えれば、そのディレクトリの追跡ファイルだけを渡す（子のディレクトリを重ねない）"
cp pkg/test_a.py pkg/sub/test_b.py
git add pkg/sub/test_b.py
make_fake_tool python3 '"-m pytest --version"*) exit 0 ;; "-m pytest "*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh" 2>&1)
assert_eq 0 $?
assert_eq $'-m pytest --version\n-m pytest test_a.py\n-m pytest test_b.py' "$(fake_log python3)"
remove_fake_tool python3

it "run-checks: scripts/run-python-tests.sh があれば自動検出より優先し、その終了コードで判定する"
mkdir -p scripts
printf '#!/bin/sh\necho custom; exit 3\n' > scripts/run-python-tests.sh
chmod +x scripts/run-python-tests.sh
out=$("$LANG_SCRIPTS/run-checks.sh" 2>&1)
assert_eq 1 $?
assert_contains "$out" "custom"
assert_contains "$out" "FAILED: python test: bash scripts/run-python-tests.sh"
chmod -x scripts/run-python-tests.sh
out=$("$LANG_SCRIPTS/run-checks.sh" 2>&1)
assert_eq 1 $?
assert_contains "$out" "実行権限が無い"
rm -r scripts pkg
git rm -rq --cached pkg

it "run-checks: ツールがなければ SKIP を出して続行する"
remove_fake_tool ruff
remove_fake_tool pytest
out=$(PATH="$(minimal_path)" "$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "SKIP: python format (ruff not found)"

it "run-checks: マーカーがなくてもステージ済みファイルの拡張子で言語を検出する"
REPO="$TEST_ROOT/checks/staged"
make_repo "$REPO"
cd "$REPO" || exit 1
echo '{ }' > x.nix && git add x.nix
make_fake_tool nixfmt '"--check"*) exit 0 ;;'
make_fake_tool statix '"check"*) exit 0 ;;'
make_fake_tool nix '*) echo "should not run without flake.nix"; exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$(fake_log nixfmt)" "--check"
assert_eq "check" "$(fake_log statix)"
assert_eq "" "$(fake_log nix)"

it "run-checks: ステージした .md は staged-checks markdown で直して検査する（作業ツリーの版を見ない）"
REPO="$TEST_ROOT/checks/md"
make_repo "$REPO"
cd "$REPO" || exit 1
echo "# doc" > doc.md && git add doc.md
make_fake_tool staged-checks '*) exit 0 ;;'
make_fake_tool markdownlint '*) echo "should not lint the worktree"; exit 1 ;;'
out=$(PATH="$(minimal_path)" "$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "markdown" "$(fake_log staged-checks)"
assert_eq "" "$(fake_log markdownlint)"
assert_contains "$out" "OK: markdown staged"
assert_contains "$out" "ALL_OK"

it "run-checks: staged-checks が違反を出せば止める"
make_fake_tool staged-checks '*) echo "Markdown lint failed"; exit 1 ;;'
out=$(PATH="$(minimal_path)" "$LANG_SCRIPTS/run-checks.sh")
assert_eq 1 $?
assert_contains "$out" "FAILED: markdown staged"

it "run-checks: staged-checks が無ければ SKIP して続行する"
remove_fake_tool staged-checks
out=$(PATH="$(minimal_path)" "$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "SKIP: markdown staged (staged-checks not found)"
make_fake_tool staged-checks '*) exit 0 ;;'

# design-records: .pre-commit-config.yaml でオプトインしたリポジトリでは、ADR の形と参照も検査する（ADR-0009）

# opt_in_design_records: 今のリポジトリに design-records の local hook の設定をコミットする
opt_in_design_records() {
  printf 'repos:\n  - repo: local\n    hooks:\n      - id: design-records\n        entry: design-records check --changed\n' \
    > .pre-commit-config.yaml
  git add .pre-commit-config.yaml && git commit -q -m "chore: opt in design-records"
}

it "run-checks: オプトインしていれば、変更した .md を渡して design-records check を走らせる"
REPO="$TEST_ROOT/checks/adr"
make_repo "$REPO"
cd "$REPO" || exit 1
opt_in_design_records
mkdir -p docs/adr
echo "# doc" > doc.md && echo "# adr" > docs/adr/0001-decide.md && git add doc.md docs/adr
make_fake_tool markdownlint '*) exit 0 ;;'
make_fake_tool design-records '"check --changed"*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "check --changed doc.md docs/adr/0001-decide.md" "$(fake_log design-records)"
assert_contains "$out" "OK: markdown design-records"
assert_contains "$out" "ALL_OK"

it "run-checks: design-records check の違反で止まる"
make_fake_tool design-records '"check --changed"*) echo "docs/adr/0001-decide.md: error: x"; exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 1 $?
assert_contains "$out" "FAILED: markdown design-records"
assert_not_contains "$out" "ALL_OK"

it "run-checks --merge: design-records check の違反で止まる"
out=$("$LANG_SCRIPTS/run-checks.sh" --merge)
assert_eq 1 $?
assert_contains "$out" "FAILED: markdown design-records"

it "run-checks: design-records がなければ SKIP を出して続行する"
remove_fake_tool design-records
out=$(PATH="$(minimal_path)" "$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "SKIP: markdown design-records (design-records not found)"
assert_contains "$out" "ALL_OK"

it "run-checks: ADR を消すだけの変更でも design-records check を走らせる"
REPO="$TEST_ROOT/checks/adr-delete"
make_repo "$REPO"
cd "$REPO" || exit 1
opt_in_design_records
mkdir -p docs/adr
echo "# a" > docs/adr/0001-a.md && echo "# b" > docs/adr/0002-b.md
git add docs/adr && git commit -q -m "docs: adr"
git rm -q docs/adr/0002-b.md
make_fake_tool design-records '"check --changed"*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "check --changed" "$(fake_log design-records)"
assert_contains "$out" "OK: markdown design-records"

it "run-checks: .md 以外の変更したファイルも design-records に渡す（置き換えられた ADR への参照を探す）"
REPO="$TEST_ROOT/checks/adr-text"
make_repo "$REPO"
cd "$REPO" || exit 1
opt_in_design_records
printf '#!/usr/bin/env bash\n# ADR-0001\n' > a.sh && git add a.sh
make_fake_tool shellcheck '*) exit 0 ;;'
make_fake_tool design-records '"check --changed"*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "check --changed a.sh" "$(fake_log design-records)"

it "run-checks: 行末のコメントや引用符のある id もオプトインと見なす"
printf 'repos:\n  - repo: local\n    hooks:\n      - id: "design-records" # ADR\n' > .pre-commit-config.yaml
git add .pre-commit-config.yaml
make_fake_tool design-records '"check --changed"*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "OK: markdown design-records"

it "run-checks: コメントアウトした id はオプトインと見なさない"
printf 'repos:\n  - repo: local\n    hooks:\n      # - id: design-records\n' > .pre-commit-config.yaml
git add .pre-commit-config.yaml
make_fake_tool design-records '*) exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "" "$(fake_log design-records)"

it "run-checks: .pre-commit-config.yaml を読めなければ、検査を飛ばさずに失敗する"
chmod 000 .pre-commit-config.yaml
make_fake_tool design-records '"check --changed"*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh" 2>&1)
assert_eq 1 $?
assert_contains "$out" ".pre-commit-config.yaml"
assert_not_contains "$out" "ALL_OK"
chmod 644 .pre-commit-config.yaml

it "run-checks: オプトインしていなければ、docs/adr があっても design-records を呼ばない"
REPO="$TEST_ROOT/checks/no-adr"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p docs/adr
echo "# doc" > doc.md && echo "# adr" > docs/adr/0001-decide.md && git add doc.md docs/adr
make_fake_tool markdownlint '*) exit 0 ;;'
make_fake_tool design-records '*) exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "" "$(fake_log design-records)"
assert_not_contains "$out" "design-records"

# fake_cargo_pwd: 偽 cargo。どのディレクトリで実行されたかを cargo.pwd に記録する
fake_cargo_pwd() {
  : > "$TEST_ROOT/cargo.pwd"
  make_fake_tool cargo '*) echo "$PWD" >> "'"$TEST_ROOT"'/cargo.pwd" ;;'
}

it "run-checks: ルートに Cargo.toml が無ければ、ステージ済みの .rs を含む crate で cargo を実行する"
# ルートで実行すると Cargo.toml が見つからず、Rust を含むコミットが必ず FAILED になっていた
REPO="$TEST_ROOT/checks/nested-rust"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p tools/a/src tools/b/src
touch tools/a/Cargo.toml tools/b/Cargo.toml
echo 'fn main() {}' > tools/a/src/main.rs && git add tools/a
echo 'fn main() {}' > tools/b/src/main.rs
fake_cargo_pwd
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq $'fmt --check\nclippy --all-targets -- -D warnings\ntest' "$(fake_log cargo)"
assert_eq "$(printf '%s\n' "$REPO/tools/a" "$REPO/tools/a" "$REPO/tools/a")" "$(cat "$TEST_ROOT/cargo.pwd")"
assert_contains "$out" "ALL_OK"

it "run-checks: 複数の crate の .rs がステージされていれば、それぞれで cargo を実行する"
git add tools/b
fake_cargo_pwd
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq 3 "$(grep -c "^$REPO/tools/a\$" "$TEST_ROOT/cargo.pwd")"
assert_eq 3 "$(grep -c "^$REPO/tools/b\$" "$TEST_ROOT/cargo.pwd")"

it "run-checks: - で始まるディレクトリの crate でも cargo を実行する（オプションと誤認しない）"
REPO="$TEST_ROOT/checks/dash-rust"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p -- -crate/src
touch -- -crate/Cargo.toml
echo 'fn main() {}' > -crate/src/main.rs && git add -- -crate
fake_cargo_pwd
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "$(printf '%s\n' "$REPO/-crate" "$REPO/-crate" "$REPO/-crate")" "$(cat "$TEST_ROOT/cargo.pwd")"

it "run-checks: crate の fmt が失敗したら、そのまま実行できる修正コマンドを示す"
REPO="$TEST_ROOT/checks/space-rust"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p "my crate/src"
touch "my crate/Cargo.toml"
echo 'fn main() {}' > "my crate/src/main.rs" && git add "my crate"
make_fake_tool cargo '"fmt --check"*) exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 1 $?
assert_contains "$out" 'FIX: (cd -- my\ crate && cargo fmt)'

it "run-checks: crate の外の .rs だけなら cargo を実行せず SKIP を出す"
REPO="$TEST_ROOT/checks/stray-rust"
make_repo "$REPO"
cd "$REPO" || exit 1
echo 'fn main() {}' > stray.rs && git add stray.rs
fake_cargo_pwd
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "" "$(fake_log cargo)"
assert_contains "$out" "SKIP: rust"
assert_contains "$out" "stray.rs"

it "run-checks: ルートに Cargo.toml があればルートで cargo を実行する"
REPO="$TEST_ROOT/checks/root-rust"
make_repo "$REPO"
cd "$REPO" || exit 1
touch Cargo.toml
fake_cargo_pwd
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "$(printf '%s\n' "$REPO" "$REPO" "$REPO")" "$(cat "$TEST_ROOT/cargo.pwd")"

it "run-checks: ステージ済みの .sh だけを shellcheck で検査する"
# 既存の .sh 全体を見ると、shellcheck を通していない既存ファイルでコミットが止まる
REPO="$TEST_ROOT/checks/shell"
make_repo "$REPO"
cd "$REPO" || exit 1
echo 'echo old' > old.sh && git add old.sh && git commit -q -m "chore: old"
printf '#!/usr/bin/env bash\necho a\n' > a.sh && git add a.sh
make_fake_tool shellcheck '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "-S warning -- a.sh" "$(fake_log shellcheck)"
assert_contains "$out" "OK: shell lint"
assert_contains "$out" "ALL_OK"

it "run-checks: shellcheck の警告で止まる"
make_fake_tool shellcheck '*) echo "SC2034 (warning)"; exit 1 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 1 $?
assert_contains "$out" "FAILED: shell lint"
assert_not_contains "$out" "ALL_OK"

# rename したファイルも、追加・変更と同じく検査する。
# pathspec での絞り込みは rename の検出より先に効くので、old.sh → new.sh のように
# 前後とも同じ拡張子の rename だけが R になり、--diff-filter=ACM から漏れていた

it "run-checks: .sh から .sh への rename を shellcheck で検査する"
REPO="$TEST_ROOT/checks/rename-sh"
make_repo "$REPO"
cd "$REPO" || exit 1
printf '#!/usr/bin/env bash\necho tool\n' > old.sh && git add old.sh && git commit -q -m "chore: old"
git mv old.sh new.sh
make_fake_tool shellcheck '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "-S warning -- new.sh" "$(fake_log shellcheck)"

it "run-checks: .md から .md への rename も Markdown の段で検査する"
REPO="$TEST_ROOT/checks/rename-md"
make_repo "$REPO"
cd "$REPO" || exit 1
printf '# notes\n\nbody\n' > old.md && git add old.md && git commit -q -m "chore: old"
git mv old.md new.md
make_fake_tool staged-checks '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "markdown" "$(fake_log staged-checks)"

it "run-checks: .rs から .rs への rename でも crate で cargo を実行する"
REPO="$TEST_ROOT/checks/rename-rs"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p tools/a/src
touch tools/a/Cargo.toml
echo 'fn main() {}' > tools/a/src/old.rs && git add tools/a && git commit -q -m "chore: crate"
git mv tools/a/src/old.rs tools/a/src/main.rs
fake_cargo_pwd
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_eq "$(printf '%s\n' "$REPO/tools/a" "$REPO/tools/a" "$REPO/tools/a")" "$(cat "$TEST_ROOT/cargo.pwd")"

# --merge: git の pre-merge-commit hook がマージ結果を検査するときのモード。
# 変更は HEAD（マージ先）と作業ツリーの差分で判定し（commit -a でマージを締めるときは
# hook に渡る index が一時ファイルなので、index に頼らない）、自動修正はしない
# （直した内容がどちらの親にもない変更としてマージコミットに紛れ込む）

it "run-checks --merge: HEAD との差分（未ステージを含む）で言語を検出する"
REPO="$TEST_ROOT/checks/merge-diff"
make_repo "$REPO"
cd "$REPO" || exit 1
printf '#!/usr/bin/env bash\necho a\n' > a.sh && git add a.sh && git commit -q -m "chore: a"
printf '#!/usr/bin/env bash\necho b\n' > a.sh
make_fake_tool shellcheck '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh" --merge)
assert_eq 0 $?
assert_eq "-S warning -- a.sh" "$(fake_log shellcheck)"
assert_contains "$out" "ALL_OK"

it "run-checks --merge: Markdown を自動修正せず、lint だけを実行する"
REPO="$TEST_ROOT/checks/merge-md"
make_repo "$REPO"
cd "$REPO" || exit 1
echo "# doc" > doc.md && git add doc.md
make_fake_tool markdownlint '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh" --merge)
assert_eq 0 $?
assert_eq "-- doc.md" "$(fake_log markdownlint)"
assert_contains "$out" "SKIP: markdown format (--merge では自動修正しない)"
assert_contains "$out" "OK: markdown lint"

it "run-checks --base: コミット済みのブランチの変更（分岐点から）で言語を検出する"
REPO="$TEST_ROOT/checks/base"
make_repo "$REPO"
cd "$REPO" || exit 1
git switch -q -c feat/x
printf '#!/usr/bin/env bash\necho a\n' > branch.sh && git add branch.sh && git commit -q -m "feat: branch"
git switch -q main
printf '#!/usr/bin/env bash\necho m\n' > main.sh && git add main.sh && git commit -q -m "feat: main"
git switch -q feat/x
make_fake_tool shellcheck '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh" --base main)
assert_eq 0 $?
assert_eq "-S warning -- branch.sh" "$(fake_log shellcheck)"
assert_contains "$out" "ALL_OK"

it "run-checks --base: Markdown を自動修正しない（--merge と同じ）"
REPO="$TEST_ROOT/checks/base-md"
make_repo "$REPO"
cd "$REPO" || exit 1
git switch -q -c feat/doc
echo "# doc" > doc.md && git add doc.md && git commit -q -m "docs: doc"
make_fake_tool markdownlint '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh" --base main)
assert_eq "-- doc.md" "$(fake_log markdownlint)"
assert_contains "$out" "SKIP: markdown format"

it "run-checks --base: 解決できない ref はエラー"
out=$("$LANG_SCRIPTS/run-checks.sh" --base nonexistent 2>&1)
assert_eq 2 $?
assert_contains "$out" "nonexistent"

it "run-checks -C: 指定した worktree の変更を、今いる場所に関係なく検査する"
REPO="$TEST_ROOT/checks/dir/app"
make_repo "$REPO"
git -C "$REPO" worktree add -q -b feat/w "$TEST_ROOT/checks/dir/app-feat-w"
printf '#!/usr/bin/env bash\necho w\n' > "$TEST_ROOT/checks/dir/app-feat-w/w.sh"
git -C "$TEST_ROOT/checks/dir/app-feat-w" add w.sh
git -C "$TEST_ROOT/checks/dir/app-feat-w" commit -q -m "feat: w"
cd "$REPO" || exit 1
make_fake_tool shellcheck '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh" -C "$TEST_ROOT/checks/dir/app-feat-w" --base main)
assert_eq 0 $?
assert_eq "-S warning -- w.sh" "$(fake_log shellcheck)"
assert_contains "$out" "ALL_OK"

it "run-checks -C: ディレクトリがなければエラー"
out=$("$LANG_SCRIPTS/run-checks.sh" -C "$TEST_ROOT/none" 2>&1)
assert_eq 2 $?
assert_contains "$out" "$TEST_ROOT/none"

it "run-checks: 知らないオプションはエラーで止まる"
out=$("$LANG_SCRIPTS/run-checks.sh" --bogus 2>&1)
assert_eq 2 $?
assert_contains "$out" "--bogus"

it "run-checks: 該当言語がなければ NOTE を出して exit 0"
REPO="$TEST_ROOT/checks/none"
make_repo "$REPO"
cd "$REPO" || exit 1
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "NOTE"

# ===========================================================================
# language-checks/scripts/fix-markdown-lint.py: コードフェンスの判定は CommonMark に従う
# ===========================================================================
# フェンスは「``` で始まる行のたびにトグル」ではなく、開きフェンスの文字（` / ~）と本数を
# 覚え、同じ文字が同じ本数以上で後ろが空白だけの行で閉じる。中の短いフェンスは内容。
# 以前は ````markdown の中の ``` の閉じフェンスを「言語のない開きフェンス」と誤認して
# ```rust に書き換え、文書を壊していた。

MD_FIXER="$LANG_SCRIPTS/fix-markdown-lint.py"
MD_DIR="$TEST_ROOT/mdfix"
mkdir -p "$MD_DIR"

it "fix-markdown-lint: \`\`\`\`markdown の中の \`\`\`typescript … \`\`\` は 1 バイトも変わらない"
printf '````markdown\n```typescript\nconst x: string = "a";\n```\n````\n' > "$MD_DIR/nested.md"
cp "$MD_DIR/nested.md" "$MD_DIR/nested.orig"
out=$(python3 "$MD_FIXER" "$MD_DIR/nested.md")
assert_eq 0 $?
assert_contains "$out" "OK:"
assert_eq "" "$(cmp "$MD_DIR/nested.md" "$MD_DIR/nested.orig" 2>&1)"

it "fix-markdown-lint: 4 本で開いた言語のない外側のブロックには言語が付き、閉じは 4 本のまま"
printf '````\nfn main() {}\n````\n' > "$MD_DIR/outer4.md"
python3 "$MD_FIXER" "$MD_DIR/outer4.md" >/dev/null
assert_eq $'````rust\nfn main() {}\n````' "$(cat "$MD_DIR/outer4.md")"

it "fix-markdown-lint: ~~~ のフェンスの中の表は整形されず、閉じ ~~~ は変わらない"
printf '~~~text\n| a | b |\n| --- | --- |\n| ccc | d |\n~~~\n' > "$MD_DIR/tilde.md"
cp "$MD_DIR/tilde.md" "$MD_DIR/tilde.orig"
python3 "$MD_FIXER" "$MD_DIR/tilde.md" >/dev/null
assert_eq "" "$(cmp "$MD_DIR/tilde.md" "$MD_DIR/tilde.orig" 2>&1)"

it "fix-markdown-lint: ~~~ の中の \`\`\` は内容で、閉じるのは ~~~ だけ"
printf '~~~\n```\nfn main() {}\n```\n~~~\n' > "$MD_DIR/tilde-inner.md"
python3 "$MD_FIXER" "$MD_DIR/tilde-inner.md" >/dev/null
assert_eq $'~~~rust\n```\nfn main() {}\n```\n~~~' "$(cat "$MD_DIR/tilde-inner.md")"

it "fix-markdown-lint: \`\`\` のみのブロックには内容から言語を推定して付ける（既存の挙動）"
printf '```\nfn main() {}\n```\n' > "$MD_DIR/plain.md"
python3 "$MD_FIXER" "$MD_DIR/plain.md" >/dev/null
assert_eq $'```rust\nfn main() {}\n```' "$(cat "$MD_DIR/plain.md")"

it "fix-markdown-lint: フェンスの外の表は整形され、フェンスの中の表は触らない（既存の挙動）"
printf '| a | b |\n| --- | --- |\n| ccc | d |\n\n```text\n| a | b |\n| --- | --- |\n| ccc | d |\n```\n' > "$MD_DIR/table.md"
python3 "$MD_FIXER" "$MD_DIR/table.md" >/dev/null
assert_eq $'| a   | b |\n| --- | - |\n| ccc | d |\n\n```text\n| a | b |\n| --- | --- |\n| ccc | d |\n```' "$(cat "$MD_DIR/table.md")"

finish

# ===========================================================================
# run-checks.sh: 排他の Slurm のジョブ中は 1 コアに限る（ADR-0041）
# ===========================================================================

REPO="$TEST_ROOT/checks/exclusive"
make_repo "$REPO"
cd "$REPO" || exit 1
echo '[package]' > Cargo.toml
echo '{}' > flake.nix
make_fake_tool cargo '*) exit 0 ;;'
make_fake_tool taskset '*) shift 2; exec "$@" ;;'
make_fake_tool nix '*) exit 0 ;;'
make_fake_tool statix '*) exit 0 ;;'
make_fake_tool nixfmt '*) exit 0 ;;'

it "run-checks: 排他のジョブが走行中なら、cargo を taskset と -j 1 で、nix flake check を --cores 1 -j 1 で走らせ、冒頭に知らせる"
make_fake_tool squeue '*) printf "NO|openmc-2K|1234\\nOK|x-shared|1235\\n" ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "排他のジョブ openmc-2K（1234）が走行中なので 1 コアで走らせる"
assert_contains "$(fake_log taskset)" "cargo clippy -j 1 --all-targets -- -D warnings"
assert_contains "$(fake_log taskset)" "cargo test -j 1"
assert_contains "$(fake_log nix)" "flake check --cores 1 -j 1"

it "run-checks: 排他のジョブが無ければ（排他でないジョブだけでも）制限しない"
make_fake_tool squeue '*) printf "OK|x-shared|1235\\n" ;;'
make_fake_tool taskset '*) shift 2; exec "$@" ;;'
make_fake_tool nix '*) exit 0 ;;'
out=$("$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_not_contains "$out" "1 コアで走らせる"
assert_eq "" "$(fake_log taskset)"
assert_contains "$(fake_log nix)" "flake check"
assert_not_contains "$(fake_log nix)" "--cores"
