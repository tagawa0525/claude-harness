#!/usr/bin/env bash
# =============================================================================
# claude-sync が、サイズと mtime が同じで中身の違うファイルも上書きすることを検証する
# =============================================================================
# ../../scripts/claude-sync.sh が対象。home-manager の activation は nix store の .claude
# （mtime が 1）から同期するので、~/.claude 側の mtime も 1 になる。語の置き換えのような
# 同じサイズの変更では、サイズと mtime の両方が前回と同じソースが来る。
# このテストは、その状態（両側の mtime 1、同じサイズ、違う中身）を作って同期し、
# CLAUDE.md、skills、scripts が新しい中身になり、commands は既存のファイルを残すことを確かめる。
#
# 使い方（リポジトリルートで実行。rsync が PATH に必要）:
#   ./modules/home/parts/tests/claude-sync.sh

set -euo pipefail

SYNC="$(cd "$(dirname "$0")/../../scripts" && pwd)/claude-sync.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

export HOME="$WORK/home"
mkdir -p "$HOME"
SRC="$WORK/src"
DEST="$HOME/.claude"

PASS=0
FAIL=0

# write <path> <content>: 中身を書き、store のパスと同じく mtime を 1 にする
write() {
  mkdir -p "$(dirname "$1")"
  printf '%s\n' "$2" > "$1"
  touch -d @1 "$1"
}

# check <説明> <path> <期待する中身>
check() {
  local desc="$1" path="$2" want="$3" got
  # ファイルが無いときも set -e で止めず、✗ に数えて残りの検査を続ける
  got=$(cat "$path" 2>/dev/null) || got="(ファイルが無い)"
  if [[ "$got" == "$want" ]]; then
    PASS=$((PASS + 1)); echo "✓ $desc"
  else
    FAIL=$((FAIL + 1)); echo "✗ $desc: expected '$want', got '$got'"
  fi
}

write "$SRC/.claude/CLAUDE.md" "ADR-0175"
write "$SRC/.claude/skills/s/SKILL.md" "ADR-0175"
write "$SRC/.claude/scripts/a.sh" "ADR-0175"
write "$SRC/.claude/commands/c.md" "source"
bash "$SYNC" "$SRC" >/dev/null

# 利用者が手で直した commands は残す（--ignore-existing）
write "$DEST/commands/c.md" "user"

# 同じサイズで中身だけ変える。~/.claude 側は 1 回目の rsync -a で mtime 1 になっている（chmod は mtime を変えない）
write "$SRC/.claude/CLAUDE.md" "ADR-NNNN"
write "$SRC/.claude/skills/s/SKILL.md" "ADR-NNNN"
write "$SRC/.claude/scripts/a.sh" "ADR-NNNN"
write "$SRC/.claude/commands/c.md" "change"
bash "$SYNC" "$SRC" >/dev/null

check "同じサイズと mtime の CLAUDE.md を上書きする" "$DEST/CLAUDE.md" "ADR-NNNN"
check "同じサイズと mtime の skills のファイルを上書きする" "$DEST/skills/s/SKILL.md" "ADR-NNNN"
check "同じサイズと mtime の scripts のファイルを上書きする" "$DEST/scripts/a.sh" "ADR-NNNN"
check "既存の commands のファイルは上書きしない" "$DEST/commands/c.md" "user"

# 引数なしでは、ハーネスの clone から同期する。nixfiles の .claude/CLAUDE.md は nixfiles のプロジェクトの指示で、
# グローバルの指示（~/.claude/CLAUDE.md）に入れると、どのリポジトリのセッションもそれを読む
write "$HOME/nix/claude-harness/.claude/CLAUDE.md" "harness"
write "$HOME/nix/nixfiles/.claude/CLAUDE.md" "nixfiles project"
rc=0
bash "$SYNC" >/dev/null || rc=$?
if (( rc == 0 )); then PASS=$((PASS + 1)); echo "✓ 引数なしの同期が成功する"; else FAIL=$((FAIL + 1)); echo "✗ 引数なしの同期が失敗した（終了コード $rc）"; fi
check "引数なしでは ~/nix/claude-harness から同期する（nixfiles のプロジェクトの指示は入れない）" "$DEST/CLAUDE.md" "harness"

# 引数なしでは、同期の前に既定の clone を git pull --ff-only で最新にする。clone が古いまま同期すると、rebuild で配られた
# 新しい版を、黙って古い版で上書きする
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
CLONE="$HOME/nix/claude-harness"
rm -rf "$CLONE"
git init -q --bare "$WORK/origin.git"
git clone -q "$WORK/origin.git" "$CLONE" 2>/dev/null
# advance <clone> <中身>: その clone で CLAUDE.md を書き換えてコミットし、origin に push する
advance() {
  mkdir -p "$1/.claude"
  printf '%s\n' "$2" > "$1/.claude/CLAUDE.md"
  git -C "$1" add .claude/CLAUDE.md
  git -C "$1" commit -qm "docs: $2"
  git -C "$1" push -q origin main
}
advance "$CLONE" "v1"
git clone -q "$WORK/origin.git" "$WORK/other"
advance "$WORK/other" "v2"

rc=0
bash "$SYNC" >"$WORK/out" 2>&1 || rc=$?
if (( rc == 0 )); then PASS=$((PASS + 1)); echo "✓ 引数なしの同期（pull つき）が成功する"; else FAIL=$((FAIL + 1)); echo "✗ 引数なしの同期（pull つき）が失敗した（終了コード $rc）"; sed 's/^/    /' "$WORK/out"; fi
check "引数なしでは、既定の clone を pull してから同期する" "$DEST/CLAUDE.md" "v2"

# stops <説明> <出力に要る文字列>: 引数なしの同期が、終了コード 1 で、stderr に理由を出して止まり、~/.claude を変えない
stops() {
  local desc="$1" want="$2" rc=0 before
  before=$(cat "$DEST/CLAUDE.md")
  bash "$SYNC" >"$WORK/stdout" 2>"$WORK/stderr" || rc=$?
  if (( rc == 1 )) && grep -qF -- "$want" "$WORK/stderr" && [[ "$(cat "$DEST/CLAUDE.md")" == "$before" ]]; then
    PASS=$((PASS + 1)); echo "✓ $desc"
  else
    FAIL=$((FAIL + 1)); echo "✗ $desc（終了コード $rc、~/.claude の CLAUDE.md は $(cat "$DEST/CLAUDE.md")）"; sed 's/^/    /' "$WORK/stderr"
  fi
}

# 引数なしの同期が入れるのは、既定のブランチの、コミットされた最新だけ。作業中の clone の中身（別のブランチ、書きかけの
# 編集）を黙って入れない。入れたいときは、パスを渡す
git -C "$CLONE" switch -qc feat/x
git -C "$CLONE" push -q -u origin feat/x 2>/dev/null
stops "既定のブランチでない clone（上流つきの feature ブランチ）では止まり、ブランチの切り替えを示す" "git -C $CLONE switch main"
git -C "$CLONE" switch -q --detach main
stops "detached HEAD の clone では止まる" "git -C $CLONE switch main"
git -C "$CLONE" switch -q main
printf 'draft\n' > "$CLONE/.claude/CLAUDE.md"
stops "書きかけの編集（.claude の未コミットの変更）がある clone では止まり、そのまま同期する方法を示す" "claude-sync $CLONE"

# 止めるのは、同期する場所（CLAUDE.md、commands、skills、scripts）の変更だけ。同期しない場所の書きかけ（メモ、テスト）では
# 止めない
git -C "$CLONE" restore .claude/CLAUDE.md
printf 'memo\n' > "$CLONE/.claude/notes.txt"
rc=0
bash "$SYNC" >"$WORK/out" 2>&1 || rc=$?
if (( rc == 0 )); then PASS=$((PASS + 1)); echo "✓ 同期しない場所の追跡していないファイルでは止まらない"; else FAIL=$((FAIL + 1)); echo "✗ 同期しない場所の追跡していないファイルで止まった（終了コード $rc）"; sed 's/^/    /' "$WORK/out"; fi
rm -f "$CLONE/.claude/notes.txt"
# 同期する場所の、追跡していない新しいファイルでは止める（書きかけのスキル）
mkdir -p "$CLONE/.claude/skills/new"
printf 'wip\n' > "$CLONE/.claude/skills/new/SKILL.md"
stops "同期する場所に追跡していないファイルがある clone では止まる" "claude-sync $CLONE"
rm -rf "$CLONE/.claude/skills"
printf 'draft\n' > "$CLONE/.claude/CLAUDE.md"

# パスを渡したときは pull しない（worktree で直しを試す、rebuild の activation、作業中の clone の中身を入れる）。
# origin は v3 に進め、clone には書きかけの draft がある
advance "$WORK/other" "v3"
bash "$SYNC" "$CLONE" >/dev/null
check "パスを渡したときは pull せず、その場所の今の中身（未コミットの編集を含む）を同期する" "$DEST/CLAUDE.md" "draft"
git -C "$CLONE" restore .claude/CLAUDE.md

# pull できないとき（手元と origin の履歴が分かれた）は、古い中身を同期せずに止める
printf 'local\n' > "$CLONE/.claude/CLAUDE.md"
git -C "$CLONE" commit -qam "docs: local"
stops "pull できないとき（履歴の分岐）は止まり、pull せずに同期する方法を示す" "claude-sync $CLONE"

# 既定のブランチは origin/HEAD から決める（中身のある origin から clone すると付く）。main でない既定のブランチでも、
# そのブランチにあれば同期し、ほかのブランチにあれば止まる
git init -q --bare -b trunk "$WORK/origin2.git"
git clone -q "$WORK/origin2.git" "$WORK/seed" 2>/dev/null
git -C "$WORK/seed" switch -qc trunk 2>/dev/null || true
mkdir -p "$WORK/seed/.claude"
printf 'trunk1\n' > "$WORK/seed/.claude/CLAUDE.md"
git -C "$WORK/seed" add .claude/CLAUDE.md
git -C "$WORK/seed" commit -qm "docs: trunk1"
git -C "$WORK/seed" push -q origin trunk
mv "$CLONE" "$WORK/clone-main"
git clone -q "$WORK/origin2.git" "$CLONE"
rc=0
bash "$SYNC" >"$WORK/out" 2>&1 || rc=$?
if (( rc == 0 )); then PASS=$((PASS + 1)); echo "✓ origin/HEAD の指す既定のブランチ（trunk）の clone の同期が成功する"; else FAIL=$((FAIL + 1)); echo "✗ origin/HEAD の指す既定のブランチ（trunk）の clone の同期が失敗した（終了コード $rc）"; sed 's/^/    /' "$WORK/out"; fi
check "origin/HEAD の指す既定のブランチの中身を同期する" "$DEST/CLAUDE.md" "trunk1"
git -C "$CLONE" switch -qc main
stops "既定のブランチが trunk の clone が main にあれば止まり、trunk への切り替えを示す" "git -C $CLONE switch trunk"
rm -rf "$CLONE"
mv "$WORK/clone-main" "$CLONE"

# GIT_DIR を継いだシェル（git の hook の中など）から呼ばれても、既定の clone を相手にする
git -C "$CLONE" reset -q --hard origin/main
git init -q "$WORK/elsewhere"
git -C "$WORK/elsewhere" commit -q --allow-empty -m "chore: init"
advance "$WORK/other" "v4"
GIT_DIR="$WORK/elsewhere/.git" bash "$SYNC" >/dev/null 2>&1 || true
check "GIT_DIR を継いでも、既定の clone を pull して同期する" "$DEST/CLAUDE.md" "v4"

# 既定の場所が、親のリポジトリの中の git でないディレクトリなら、親を pull せずにそのまま同期する
rm -rf "$CLONE"
git init -q "$HOME/nix"
write "$CLONE/.claude/CLAUDE.md" "plain copy"
rc=0
bash "$SYNC" >/dev/null 2>&1 || rc=$?
if (( rc == 0 )); then PASS=$((PASS + 1)); echo "✓ git でない既定の場所の同期が成功する"; else FAIL=$((FAIL + 1)); echo "✗ git でない既定の場所の同期が失敗した（終了コード $rc）"; fi
check "git でない既定の場所は、親のリポジトリを pull せずにそのまま同期する" "$DEST/CLAUDE.md" "plain copy"

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
