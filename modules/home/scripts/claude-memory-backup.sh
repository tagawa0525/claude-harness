#!/usr/bin/env bash
# =============================================================================
# Claude Code Memory Backup
# =============================================================================
# Claude Code の自動メモリ（<claude-dir>/projects/*/memory/ の下）を、<claude-dir> の git の ref
# refs/memory-backup にコミットする。refs/heads の下に置かないのは、ブランチとして checkout すると、main で
# 追跡しているファイル（settings.json など）が作業ツリーから消えるため。Claude Code の Stop の hook（応答の終わりごと）から呼ばれる
# （登録は modules/home/parts/claude-code.nix）。中身が前のコミットと同じならコミットしない。
#
# Usage: claude-memory-backup [claude-dir]
#   claude-dir 省略時: ~/.claude
#   標準入力に hook の JSON があれば、session_id と cwd をコミットの本文に入れる
#
# 戻し方の例:
#   git -C ~/.claude log --stat refs/memory-backup -- projects/<プロジェクト>/memory/<ファイル>
#   git -C ~/.claude show refs/memory-backup~3:projects/<プロジェクト>/memory/<ファイル>
#
# 失敗したときは、1 行目に理由を書いて終了コード 1 で終わる。Claude Code は Stop の hook の 0 と 2 以外の
# 終了コードを止めない失敗として扱い、標準エラーの 1 行目を通知に出す（2 は応答を続けさせるので使わない）。

set -euo pipefail

CLAUDE_DIR="${1:-$HOME/.claude}"
REF=refs/memory-backup

# hook の JSON。端末から手で呼んだときは読まない
INPUT='{}'
if [ ! -t 0 ]; then
  INPUT=$(cat)
fi

backup() {
  # hook は親の環境を継ぐ。GIT_DIR などを export したシェル（git の hook の中、dotfiles の別名）から起動した
  # Claude Code では、以下の git が claude-dir でなくそのリポジトリと index を相手にする
  while IFS= read -r var; do unset "$var"; done < <(git rev-parse --local-env-vars)
  cd "$CLAUDE_DIR"
  local top
  top=$(git rev-parse --show-toplevel)
  # 上の階層のリポジトリ（ホームを丸ごと追跡するものなど）に、知らずに ref を作らない
  if [ "$top" != "$(pwd -P)" ]; then
    echo "$CLAUDE_DIR はリポジトリの直下ではありません（直下: $top）" >&2
    return 1
  fi

  local gitdir
  gitdir=$(git rev-parse --absolute-git-dir)
  # 同時に終わった応答の hook どうしで、同じ親から 2 つのコミットを作って片方の変更を失わないよう、
  # 親を読んでから ref を進めるまでを 1 つずつにする
  exec 9>"$gitdir/memory-backup.lock"
  flock -w 30 9

  # 作業ツリーの index でなく一時の index に足す。利用者がステージした変更と、main の HEAD に触らない
  # local にしない: EXIT の trap は関数を抜けた後のサブシェルの終わりに走る
  TMP=$(mktemp -d "$gitdir/memory-backup-tmp.XXXXXX")
  trap 'rm -rf "$TMP"' EXIT
  export GIT_INDEX_FILE="$TMP/index"

  shopt -s nullglob
  local mems=(projects/*/memory)
  shopt -u nullglob
  # symlink のメモリのディレクトリは、git add がリンク（mode 120000）だけを入れて中身を退避しないので、成功と
  # 見せずに止める。リンクを追うと、リポジトリの外のファイルまで取り込める
  local m
  for m in "${mems[@]}"; do
    if [ -L "$m" ]; then
      echo "$CLAUDE_DIR/$m は symlink で、中身を退避できません" >&2
      return 1
    fi
  done
  if [ "${#mems[@]}" -gt 0 ]; then
    # -f: ~/.claude の .gitignore（手で管理していて、ホストごとに無いこともある）に頼らず、メモリだけを入れる
    git add -f -- "${mems[@]}"
  fi
  local tree parent
  tree=$(git write-tree)
  parent=$(git rev-parse -q --verify "$REF^{commit}" || true)
  if [ -z "$parent" ] && [ "${#mems[@]}" -eq 0 ]; then
    return 0
  fi
  if [ -n "$parent" ] && [ "$(git rev-parse "$parent^{tree}")" = "$tree" ]; then
    return 0
  fi

  local session cwd commit
  session=$(jq -r '.session_id // "-"' <<<"$INPUT")
  cwd=$(jq -r '.cwd // "-"' <<<"$INPUT")
  # git commit でなく commit-tree と update-ref で refs/memory-backup だけを進める。git commit は HEAD のブランチ
  # （~/.claude では main）にコミットし、main への直接のコミットを止める pre-commit の hook と、メッセージと
  # 中身の検査（commit-msg、機密情報、日本語の語）に掛かる。バックアップは壊れた中身もそのまま残すのが目的で、
  # 検査で止まると、その間の版が残らない
  commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} -F - <<EOF
memory: snapshot

session: $session
cwd: $cwd
EOF
  )
  # 古い値に親（無ければ空 = まだ無いこと）を渡し、ロックの外で動かされていたら失敗させる
  git update-ref -m "claude-memory-backup" "$REF" "$commit" "$parent"
}

LOG=$(mktemp)
trap 'rm -f "$LOG"' EXIT
# サブシェルの中で set -e を効かせる（if や || の条件の中で呼ぶと、関数の中の set -e が効かない）
set +e
(
  set -e
  backup
) 2>"$LOG"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "claude-memory-backup: メモリを $CLAUDE_DIR の refs/memory-backup にコミットできませんでした（この応答の版は残っていません）" >&2
  cat "$LOG" >&2
  exit 1
fi
