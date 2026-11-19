#!/usr/bin/env bash
# 取り込む側のリポジトリ（nixfiles）の検査 harness-contract.sh を、このスクリプトのある worktree の HEAD の版の
# ハーネスで実行する。その検査は、配備（nix-rebuild update）が lock を進めた後に実行され、落ちると配備が止まる。マージの
# 前にここで実行して、配備で初めて分かることを無くす（照合する約束は、nixfiles の
# modules/home/parts/tests/harness-contract.sh の冒頭）。
#
# 使い方:
#   scripts/check-nixfiles-contract.sh [host]
#     host: 検査する nixosConfigurations の名前。既定はこの機械のホスト名
#   NIXFILES_DIR: nixfiles の clone の場所。既定は ~/nix/nixfiles
#   NIXFILES_REF: nixfiles の側の版（clone にあるブランチかコミット）。既定は origin の既定ブランチを fetch した版。
#     ハーネスが約束を変える PR と対になる nixfiles の修正を、マージの前にブランチのまま確かめるときに渡す
#
# 見るのは HEAD のコミットだけ（nix が git から取り出す）。作業ツリーの変更は検査に入らないので、あれば NOTE で知らせる。
# 配備されるのはマージの結果なので、既定ブランチから遅れたブランチは rebase してから実行する。
# nixfiles の側は一時の worktree に取り出し、flake.lock のハーネスだけをこの HEAD に向ける（nixfiles の clone の
# 作業ツリーとブランチには触れない）。
#
# 出力と終了コード:
#   検査の出力（✓ と ✗ の行）をそのまま出す
#   OK: …      検査が通った（0）
#   FAILED: …  検査が ✗ の行を出して落ちた（1）。✗ の行が約束の食い違い
#   ERROR: …   検査を実行できない（2）。nixfiles の clone が無い、lock を向けられない、構成を評価できないなど。
#              通ったことにはしない（clone の無い機械では、clone のある機械で実行する）
set -uo pipefail

die() {
  echo "ERROR: $*" >&2
  exit 2
}

# 呼んだ場所（cwd）でなく、スクリプトのある worktree を検査する（nixfiles の中から呼んでも取り違えない）
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
nixfiles="${NIXFILES_DIR:-$HOME/nix/nixfiles}"
host="${1:-$(hostname)}"
ref="${NIXFILES_REF:-}"
contract=modules/home/parts/tests/harness-contract.sh

[[ -e "$nixfiles/.git" ]] || die "nixfiles の clone がありません（$nixfiles）。検査は実行していません。clone のある機械で実行するか、NIXFILES_DIR で場所を渡してください"
head=$(git -C "$repo" rev-parse HEAD) || die "$repo の HEAD を読めません"
if [[ -n "$(git -C "$repo" status --porcelain)" ]]; then
  echo "NOTE: 作業ツリーに変更があります。検査するのはコミットした版（${head:0:8}）だけです"
fi

if [[ -z "$ref" ]]; then
  git -C "$nixfiles" fetch -q origin || die "nixfiles の origin を fetch できません（$nixfiles）"
  ref=$(git -C "$nixfiles" symbolic-ref --quiet --short refs/remotes/origin/HEAD) ||
    die "nixfiles の既定ブランチが分かりません（$nixfiles で git remote set-head origin -a を実行してください）"
fi

work=$(mktemp -d) || die "一時のディレクトリを作れません"
cleanup() {
  git -C "$nixfiles" worktree remove --force "$work/nixfiles" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

git -C "$nixfiles" worktree add -q --detach "$work/nixfiles" "$ref" || die "nixfiles の $ref を取り出せません（$nixfiles）"
cd "$work/nixfiles" || die "一時の worktree に移れません"
[[ -f "$contract" ]] || die "nixfiles の $ref に $contract がありません"

echo "==> nixfiles（$ref $(git rev-parse --short HEAD)）の lock のハーネスを、$repo の ${head:0:8} に向ける"
nix flake lock --option warn-dirty false --override-input claude-harness "git+file://$repo?rev=$head" ||
  die "lock のハーネスをこの版に向けられません（nix flake lock の失敗。上の nix の error の行が原因）"

if bash "$contract" "$host" 2>&1 | tee "$work/contract.out"; then
  echo "OK: nixfiles の検査が、${head:0:8} の版で通りました"
elif grep -q '^✗' "$work/contract.out"; then
  echo "FAILED: nixfiles の検査が、${head:0:8} の版で落ちました（✗ の行が食い違い）。ハーネスが約束を変えたなら nixfiles を合わせる PR を先に出し（確かめるのは NIXFILES_REF）、検査の前提が古ければ検査を直します" >&2
  exit 1
else
  die "nixfiles の検査を最後まで実行できませんでした（✗ の行が無い。構成 nixosConfigurations.$host を評価できないか、ビルドの失敗。上の nix の error の行が原因）"
fi
