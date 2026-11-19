#!/usr/bin/env bash
# scripts/check-nixfiles-contract.sh（nixfiles の検査を、このブランチの版のハーネスで実行する）のテスト
#
# 実行: bash claude/tests/scripts/check-nixfiles-contract.sh（まとめて実行: bash claude/tests/scripts.sh）

# nix と nixfiles は偽物にする。偽の nixfiles の検査は、環境変数で渡した行を出し、渡した終了コードで終わる
source "$(dirname "$0")/../lib.sh"

# スクリプトは自分のある worktree を検査するので、一時のハーネスのリポジトリにコピーして呼ぶ（このリポジトリのブランチの
# 状態と、.git の無いコピーからの実行に左右されない）
REPO="$TEST_ROOT/harness"
make_repo "$REPO"
make_remote "$REPO"
git -C "$REPO" remote set-head origin main
git -C "$REPO" switch -q -c feat/x main
mkdir -p "$REPO/scripts"
cp "$CLAUDE_DIR/../scripts/check-nixfiles-contract.sh" "$REPO/scripts/"
git -C "$REPO" add scripts
git -C "$REPO" commit -q -m "feat: x"
CHECK="$REPO/scripts/check-nixfiles-contract.sh"
HEAD_SHA=$(git -C "$REPO" rev-parse HEAD)

NIXFILES="$TEST_ROOT/nixfiles"
make_repo "$NIXFILES"
mkdir -p "$NIXFILES/modules/home/parts/tests"
cat >"$NIXFILES/modules/home/parts/tests/harness-contract.sh" <<'EOF'
#!/usr/bin/env bash
echo "host=$1 pwd=$PWD"
printf '%s\n' "${FAKE_CONTRACT_OUT:-✓ 約束どおり}"
exit "${FAKE_CONTRACT_EXIT:-0}"
EOF
git -C "$NIXFILES" add modules
git -C "$NIXFILES" commit -q -m "test: contract"
make_remote "$NIXFILES"
git -C "$NIXFILES" remote set-head origin main
export NIXFILES_DIR="$NIXFILES"

# 偽の nix flake lock は、渡された rev を cwd の flake.lock に書く（FAKE_NIX_KEEP_LOCK があれば書かずに 0 で終わる。
# 本物の nix は、上書きする input が flake に無いと、警告だけで lock を変えずに 0 で終わる）
# shellcheck disable=SC2016  # 偽物の本文の中で展開する
make_fake_tool nix '"flake lock "*)
    [[ "${FAKE_NIX_EXIT:-0}" == 0 ]] || exit "$FAKE_NIX_EXIT"
    args="$*"
    [[ -n "${FAKE_NIX_KEEP_LOCK:-}" ]] || printf "{\"nodes\":{\"claude-harness\":{\"locked\":{\"rev\":\"%s\"}}}}\n" "${args##*rev=}" >flake.lock ;;'
PATH="$TEST_ROOT/fakebin:$PATH"

it "検査が通れば OK を出して 0 で終わり、lock のハーネスをこのリポジトリの HEAD に向ける"
out=$("$CHECK" testhost 2>&1)
assert_eq 0 $?
assert_contains "$out" "✓ 約束どおり"
assert_contains "$out" "host=testhost"
assert_contains "$out" "OK: "
assert_contains "$(fake_log nix)" "--override-input claude-harness git+file://$REPO?rev=$HEAD_SHA"

it "nixfiles の clone の作業ツリーと worktree の一覧に跡を残さない"
assert_eq 1 "$(git -C "$NIXFILES" worktree list | wc -l)"
assert_eq "" "$(git -C "$NIXFILES" status --porcelain)"

it "検査は nixfiles の clone でなく、一時の worktree で実行する"
assert_not_contains "$out" "pwd=$NIXFILES"

it "✗ の行を出して落ちた検査は、FAILED を出して 1 で終わる"
out=$(FAKE_CONTRACT_OUT="✗ 入口が違う" FAKE_CONTRACT_EXIT=1 "$CHECK" testhost 2>&1)
assert_eq 1 $?
assert_contains "$out" "✗ 入口が違う"
assert_contains "$out" "FAILED: "

it "✗ の行が無いまま落ちた検査は、約束の食い違いと言わず、ERROR を出して 2 で終わる"
out=$(FAKE_CONTRACT_OUT="error: flake does not provide attribute" FAKE_CONTRACT_EXIT=1 "$CHECK" testhost 2>&1)
assert_eq 2 $?
assert_contains "$out" "ERROR: "
assert_not_contains "$out" "FAILED: "

it "lock を向けられなければ、検査を呼ばずに ERROR を出して 2 で終わる"
out=$(FAKE_NIX_EXIT=1 "$CHECK" testhost 2>&1)
assert_eq 2 $?
assert_contains "$out" "ERROR: "
assert_not_contains "$out" "host=testhost"
assert_eq 1 "$(git -C "$NIXFILES" worktree list | wc -l)"

it "lock のハーネスがこの HEAD にならなければ、古い版を検査して OK を出さず、ERROR を出して 2 で終わる"
out=$(FAKE_NIX_KEEP_LOCK=1 "$CHECK" testhost 2>&1)
assert_eq 2 $?
assert_contains "$out" "ERROR: "
assert_not_contains "$out" "host=testhost"

it "nixfiles の clone が無ければ、通ったことにせず ERROR を出して 2 で終わる"
out=$(NIXFILES_DIR="$TEST_ROOT/missing" "$CHECK" testhost 2>&1)
assert_eq 2 $?
assert_contains "$out" "ERROR: "
assert_contains "$out" "$TEST_ROOT/missing"

it "ほかのリポジトリの中から呼んでも、スクリプトのあるリポジトリの HEAD を検査する"
: >"$TEST_ROOT/nix.log"
out=$(cd "$NIXFILES" && "$CHECK" testhost 2>&1)
assert_eq 0 $?
assert_contains "$(fake_log nix)" "git+file://$REPO?rev=$HEAD_SHA"

it "NIXFILES_REF で、nixfiles の側の版（検査を直したブランチ）を選べる"
git -C "$NIXFILES" switch -q -c fix/contract
# shellcheck disable=SC2016  # sed に渡す文字のまま
sed -i 's/host=\$1/fixed host=$1/' "$NIXFILES/modules/home/parts/tests/harness-contract.sh"
git -C "$NIXFILES" commit -q -am "fix: contract"
git -C "$NIXFILES" switch -q main
out=$(NIXFILES_REF=fix/contract "$CHECK" testhost 2>&1)
assert_eq 0 $?
assert_contains "$out" "fixed host=testhost"
out=$("$CHECK" testhost 2>&1)
assert_not_contains "$out" "fixed host=testhost"

it "無い NIXFILES_REF は、ERROR を出して 2 で終わる"
out=$(NIXFILES_REF=no-such-branch "$CHECK" testhost 2>&1)
assert_eq 2 $?
assert_contains "$out" "ERROR: "

it "host を省くと、配備と同じく HOSTNAME の構成を検査する"
out=$(HOSTNAME=fromenv "$CHECK" 2>&1)
assert_eq 0 $?
assert_contains "$out" "host=fromenv"

it "git の hook から呼ばれても（GIT_DIR が渡っても）、nixfiles と自分のリポジトリを取り違えない"
out=$(GIT_DIR="$TEST_ROOT/elsewhere/.git" "$CHECK" testhost 2>&1)
assert_eq 0 $?
assert_contains "$out" "OK: "

it "作業ツリーに変更があれば、検査するのがコミットした版だけだと知らせる"
echo x >"$REPO/untracked"
out=$("$CHECK" testhost 2>&1)
assert_eq 0 $?
assert_contains "$out" "NOTE: 作業ツリーに変更があります"
rm "$REPO/untracked"

it "既定ブランチから遅れたブランチは、検査せずに ERROR を出して 2 で終わる（配備されるのはマージの結果）"
git -C "$REPO" switch -q main
commit_file "$REPO" other.txt "feat: other"
git -C "$REPO" push -q origin main
git -C "$REPO" switch -q feat/x
out=$("$CHECK" testhost 2>&1)
assert_eq 2 $?
assert_contains "$out" "ERROR: "
assert_contains "$out" "rebase"
assert_not_contains "$out" "host=testhost"

it "ハーネスの origin を fetch できなければ、古い版と比べて通さず、ERROR を出して 2 で終わる"
git -C "$REPO" rebase -q origin/main
git -C "$REPO" remote set-url origin "$TEST_ROOT/gone.git"
out=$("$CHECK" testhost 2>&1)
assert_eq 2 $?
assert_contains "$out" "ERROR: "
assert_contains "$out" "fetch"
assert_not_contains "$out" "host=testhost"

finish
