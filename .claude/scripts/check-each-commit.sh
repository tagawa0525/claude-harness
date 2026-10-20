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
# 終了コード: 全コミットで 0 なら 0、1 つでも失敗すれば 1、使い方の誤りは 2、
# 実行時の環境の失敗は 3（git rev-list、一時ディレクトリ、worktree の作成、checkout、clean の失敗。
# shallow clone や壊れたオブジェクトなど）。clean に失敗したコミットは検査しない
# （前のコミットの副産物が残ったまま通ると、誤って exit=0 と出る）。
# 失敗しても止まらず全コミットを走らせる。
#
# 使いどころ: 依存や lock を変えるブランチなど、各コミットでビルドが通るべきとき
# （`git bisect` が使えなくなる）。例:
#   check-each-commit.sh origin/main -- nix build .#default
#
# 現在の worktree と stash には触れず、`git worktree add --detach` の一時 worktree で
# 調べる。一時 worktree は終了時（失敗、中断を含む）に必ず消す。
#
# 許可リスト（modules/home/parts/claude-code.nix）に入れない。`-- <任意のコマンド>` を
# 実行するので、プレフィックス一致で事前承認すると何でも確認なしで走ってしまう。

set -uo pipefail

# git の hook や GIT_DIR を export したシェルから呼ばれると、git -C "$wt" の checkout と
# clean が一時 worktree でなく呼び出し元の作業ツリーに作用して消しうる。検査コマンドにも渡さない
while IFS= read -r var; do unset "$var"; done < <(git rev-parse --local-env-vars)

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

# プロセス置換では rev-list の失敗が消えるので、いったん変数に受けて終了コードを見る
list=$(git rev-list --topo-order --reverse "$base..HEAD") || {
  echo "git rev-list に失敗しました（shallow clone や壊れたオブジェクトの可能性）: $base..HEAD" >&2
  exit 3
}
mapfile -t commits <<<"$list"
[[ -n "$list" ]] || commits=()
((${#commits[@]} > 0)) || {
  echo "$base..HEAD にコミットがありません" >&2
  exit 2
}

tmp=$(mktemp -d "${TMPDIR:-/tmp}/check-each-commit.XXXXXX") || {
  echo "一時ディレクトリを作れません: ${TMPDIR:-/tmp}" >&2
  exit 3
}
wt="$tmp/wt"
cleanup() {
  # 検査が書き込み不可のディレクトリ（Go のモジュールキャッシュなど）を残すと rm が失敗する
  chmod -R u+w "$tmp" 2>/dev/null
  git worktree remove --force "$wt" 2>/dev/null
  rm -rf "$tmp"
  git worktree prune
  [[ ! -e "$tmp" ]] || echo "一時ディレクトリを消せませんでした: $tmp" >&2
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

git worktree add -q --detach "$wt" "${commits[0]}" || exit 3

rc=0
for sha in "${commits[@]}"; do
  git -C "$wt" checkout -q --detach "$sha" || exit 3
  # 検査の副産物（ビルド結果など）が次のコミットに残らないようにする
  chmod -R u+w "$wt" 2>/dev/null
  git -C "$wt" clean -qffdx || {
    echo "git clean に失敗したので $sha を検査しません" >&2
    exit 3
  }
  # 標準入力を渡さない（パイプラインやエージェントのシェルの入力を食わない）
  (cd "$wt" && "$@") >&2 </dev/null
  status=$?
  ((status == 0)) || rc=1
  printf '%s %s exit=%d\n' "$(git rev-parse --short "$sha")" "$(git log -1 --format=%s "$sha")" "$status"
done
exit "$rc"
