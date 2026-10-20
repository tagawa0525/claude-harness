#!/usr/bin/env bash
# check-each-commit.sh — <base>..HEAD の各コミットで検査コマンドを走らせる
#
# Usage: check-each-commit.sh <base> -- <コマンド…>
#   base      比較元（git の rev）。<base>..HEAD のコミットを古い順に調べる
#   コマンド  各コミットをチェックアウトした一時 worktree の根で走らせる。
#             何を通すかは呼び出し側が決める（RED を skip でコミットする運用では
#             「テストが全部通る」は求められず、ビルドだけを通す、など）。
#             コマンドの出力は標準エラーに回し、標準出力は 1 コミット 1 行にする
#
# 出力（標準出力）: `<短縮hash> <件名> exit=<N>`
# 終了コード: 全コミットで 0 なら 0、1 つでも失敗すれば 1、使い方の誤りは 2。
# 失敗しても止まらず全コミットを走らせる。
#
# 使いどころ: 依存や lock を変えるブランチなど、各コミットでビルドが通るべきとき
# （`git bisect` が使えなくなる）。例:
#   check-each-commit.sh origin/main -- nix build .#default
#
# 現在の worktree と stash には触れず、`git worktree add --detach` の一時 worktree で
# 調べる。一時 worktree は終了時（失敗、中断を含む）に必ず消す。

set -uo pipefail

usage() {
  echo "Usage: ${0##*/} <base> -- <コマンド…>" >&2
  exit 2
}

(($# >= 3)) && [[ "$2" == "--" ]] || usage
base=$1
shift 2

git rev-parse --verify -q "$base^{commit}" >/dev/null || {
  echo "base を解決できません: $base" >&2
  exit 2
}

mapfile -t commits < <(git rev-list --reverse "$base..HEAD")
((${#commits[@]} > 0)) || {
  echo "$base..HEAD にコミットがありません" >&2
  exit 2
}

tmp=$(mktemp -d "${TMPDIR:-/tmp}/check-each-commit.XXXXXX")
wt="$tmp/wt"
cleanup() {
  git worktree remove --force "$wt" 2>/dev/null
  rm -rf "$tmp"
  git worktree prune
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

git worktree add -q --detach "$wt" "${commits[0]}" || exit 2

rc=0
for sha in "${commits[@]}"; do
  git -C "$wt" checkout -q --detach "$sha" || exit 2
  # 検査の副産物（ビルド結果など）が次のコミットに残らないようにする
  git -C "$wt" clean -qfdx
  (cd "$wt" && "$@") >&2
  status=$?
  ((status == 0)) || rc=1
  printf '%s %s exit=%d\n' "$(git rev-parse --short "$sha")" "$(git log -1 --format=%s "$sha")" "$status"
done
exit "$rc"
