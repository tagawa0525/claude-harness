#!/usr/bin/env bash
# language-checks/scripts（run-checks.sh / fix-markdown-lint.py） のテスト
#
# 実行: bash .claude/tests/scripts/language-checks.sh（まとめて実行: bash .claude/tests/scripts.sh）
#
# 実物の gh・ruff 等は使わず、make_fake_tool で PATH 先頭に置いた偽コマンドで
# 「スクリプトが何を呼び、出力をどう判定するか」を検証する。

source "$(dirname "$0")/../lib.sh"

LANG_SCRIPTS="$CLAUDE_DIR/skills/language-checks/scripts"

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

it "run-checks: Markdown の補完修正は python3 がなければ SKIP して続行する"
REPO="$TEST_ROOT/checks/md"
make_repo "$REPO"
cd "$REPO" || exit 1
echo "# doc" > doc.md && git add doc.md
make_fake_tool markdownlint '*) exit 0 ;;'
out=$(PATH="$(minimal_path)" "$LANG_SCRIPTS/run-checks.sh")
assert_eq 0 $?
assert_contains "$out" "SKIP: markdown fixer (python3 not found)"
assert_contains "$out" "ALL_OK"

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
