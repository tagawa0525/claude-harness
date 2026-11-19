#!/usr/bin/env bash
# 取り込む側のリポジトリ（nixfiles）の検査 harness-contract.sh を、このスクリプトのある worktree の HEAD の版の
# ハーネスで実行する。その検査は、配備（nix-rebuild update）が lock を進めた後に実行され、落ちると配備が止まる。マージの
# 前にここで実行して、配備で初めて分かることを無くす（照合する約束は、nixfiles の
# modules/home/parts/tests/harness-contract.sh の冒頭）。
#
# 使い方:
#   scripts/check-nixfiles-contract.sh [host]
#     host: 検査する nixosConfigurations の名前。既定は、配備（nix-rebuild update）が渡すのと同じ $HOSTNAME
#   NIXFILES_DIR: nixfiles の clone の場所。既定は ~/nix/nixfiles
#   NIXFILES_REF: nixfiles の側の版（clone にあるブランチかコミット）。既定は origin の既定ブランチを fetch した版。
#     ハーネスが約束を変える PR と対になる nixfiles の修正を、マージの前にブランチのまま確かめるときに渡す
#
# 見るのは HEAD のコミットだけ（nix が git から取り出す）。作業ツリーの変更は検査に入らないので、あれば NOTE で知らせる。
# 配備されるのはマージの結果なので、既定ブランチ（origin）から遅れたブランチは検査せずに止め、rebase を求める。
# nixfiles の側は一時の worktree に取り出し、flake.lock のハーネスだけをこの HEAD に向ける（nixfiles の clone の
# 作業ツリーとブランチには触れない）。
#
# 出力と終了コード:
#   検査の出力（✓ と ✗ の行）をそのまま出す
#   OK: …      検査が通った（0）
#   FAILED: …  検査が ✗ の行を出して落ちた（1）。✗ の行が原因（約束の食い違いか、検査の中の配る手順の失敗）
#   ERROR: …   検査を実行できない（2）。nixfiles の clone が無い、ブランチが遅れている、lock を向けられない、構成を
#              評価できないなど。
#              通ったことにはしない（clone の無い機械では、clone のある機械で実行する）
set -uo pipefail

# git の hook から呼ばれると、git が GIT_DIR などを渡す。残すと、下の git -C が別のリポジトリを相手に動く
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX GIT_COMMON_DIR

die() {
  echo "ERROR: $*" >&2
  exit 2
}

# 呼んだ場所（cwd）でなく、スクリプトのある worktree を検査する（nixfiles の中から呼んでも取り違えない）
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
nixfiles="${NIXFILES_DIR:-$HOME/nix/nixfiles}"
host="${1:-${HOSTNAME:-}}"
ref="${NIXFILES_REF:-}"
contract=modules/home/parts/tests/harness-contract.sh

[[ -e "$nixfiles/.git" ]] || die "nixfiles の clone がありません（$nixfiles）。検査は実行していません。clone のある機械で実行するか、NIXFILES_DIR で場所を渡してください"
[[ -n "$host" ]] || die "検査する構成の名前が分かりません（HOSTNAME が空）。引数で渡してください"
head=$(git -C "$repo" rev-parse HEAD) || die "$repo の HEAD を読めません"
if [[ -n "$(git -C "$repo" status --porcelain)" ]]; then
  echo "NOTE: 作業ツリーに変更があります。検査するのはコミットした版（${head:0:8}）だけです"
fi

# fetch できないまま手元の origin の版と比べると、遅れたブランチを通しうるので止める
git -C "$repo" fetch -q origin || die "ハーネスの origin を fetch できません（$repo）。既定ブランチからの遅れを確かめられないので、検査していません"
if harness_base=$(git -C "$repo" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null); then
  git -C "$repo" merge-base --is-ancestor "$harness_base" "$head" ||
    die "このブランチは $harness_base から遅れています。配備されるのはマージの結果なので、rebase してから実行してください"
else
  echo "NOTE: ハーネスの既定ブランチが分からないので、ブランチの遅れは確かめていません"
fi

if [[ -z "$ref" ]]; then
  git -C "$nixfiles" fetch -q origin || die "nixfiles の origin を fetch できません（$nixfiles）"
  ref=$(git -C "$nixfiles" symbolic-ref --quiet --short refs/remotes/origin/HEAD) ||
    die "nixfiles の既定ブランチが分かりません（$nixfiles で git remote set-head origin -a を実行してください）"
fi

work=$(mktemp -d) || die "一時のディレクトリを作れません"
cleanup() {
  if [[ -e "$work/nixfiles" ]]; then
    git -C "$nixfiles" worktree remove --force "$work/nixfiles" ||
      echo "WARN: nixfiles の一時の worktree を外せませんでした（$nixfiles で git worktree prune を実行してください）" >&2
  fi
  rm -rf "$work"
}
trap cleanup EXIT

git -C "$nixfiles" worktree add -q --detach "$work/nixfiles" "$ref" || die "nixfiles の $ref を取り出せません（$nixfiles）"
cd "$work/nixfiles" || die "一時の worktree に移れません"
[[ -f "$contract" ]] || die "nixfiles の $ref に $contract がありません"

echo "==> nixfiles（$ref $(git rev-parse --short HEAD)）の lock のハーネスを、$repo の ${head:0:8} に向ける"
nix flake lock --option warn-dirty false --override-input claude-harness "git+file://$repo?rev=$head" ||
  die "lock のハーネスをこの版に向けられません（nix flake lock の失敗。上の nix の error の行が原因）"
# nix は、上書きする input が flake に無いと、警告だけで lock を変えずに 0 で終わる。前の lock の版を検査して OK を
# 出さないよう、向いた先を確かめる
locked=$(jq -r '.nodes["claude-harness"].locked.rev // ""' flake.lock) || die "nixfiles の flake.lock を読めません"
[[ "$locked" == "$head" ]] ||
  die "lock のハーネスがこの版に向いていません（lock は ${locked:-なし}）。nixfiles の flake の input の名前が claude-harness でなくなっていれば、このスクリプトを合わせてください"

if bash "$contract" "$host" 2>&1 | tee "$work/contract.out"; then
  echo "OK: nixfiles の検査が、${head:0:8} の版で通りました"
elif grep -q '^✗' "$work/contract.out"; then
  echo "FAILED: nixfiles の検査が、${head:0:8} の版で落ちました（✗ の行が原因）。ハーネスが約束を変えたなら nixfiles を合わせる PR を先に出し（確かめるのは NIXFILES_REF）、検査の前提が古ければ検査を直します" >&2
  exit 1
else
  die "nixfiles の検査を最後まで実行できませんでした（✗ の行が無い。構成 nixosConfigurations.$host を評価できないか、ビルドの失敗。上の nix の error の行が原因）"
fi
