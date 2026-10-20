#!/usr/bin/env bash
# worktree-add.sh — 命名規則に従って worktree を作る
#
# Usage: worktree-add.sh <branch> [--from <ref>] [--carry-changes]
#
# 作成先はメイン worktree の親ディレクトリの `<repo>-<branch>`（ブランチ名の / は - に変換）。
# ブランチの状態は自動判定する:
#   - ローカルにある       → そのままチェックアウト
#   - リモートにだけある   → 追跡ブランチを作ってチェックアウト
#   - どこにもない         → 既定ブランチ（origin/HEAD → origin/main → origin/master → main → master
#                            の最初にあるもの）から上流なしで新規作成。今の HEAD から作ると、
#                            作業中のブランチの未完了のコミットが別のトピックに入るため
# --from <ref>: 新規作成の起点を明示する（今のブランチの続きなら --from HEAD）。
#   既存のブランチに付けたらエラー。
# --carry-changes: 未コミットの変更（未追跡ファイル含む）を stash 経由で worktree 側に移す
#   （git-commit スキルの「main からの退避」）。
#
# 共有（.worktreelink、ADR-0016）: 新しい worktree のルートに .worktreelink があれば、1 行 1 パス
# （ルートからの相対パス。空行と # で始まる行は読み飛ばし、前後に空白のある行はエラー）を、メイン worktree の同じパスへの
# 絶対パスの symlink として作る。git で追跡しない大きなデータ（キャッシュ・参照データ）を
# worktree の間でコピーせず共有するため。
#   - パスはリテラルだけ。絶対パス、. / .. を含むもの、ワイルドカード（* ? [）と \、! か : で始まるものはエラー
#   - 同じパスや入れ子のパス、親が新しい worktree でディレクトリでない（ファイルか symlink）パスはエラー
#   - メイン worktree で追跡されているパスはエラー（新しい worktree での書き込みがメインの変更になる）
#   - 新しい worktree で git に無視されていなければエラー（symlink が未追跡ファイルに見える）。
#     symlink はディレクトリではないので、`dir/` のようにディレクトリだけに一致するパターンでは
#     無視されない。判定は symlink を作る前の、そのパスが無い状態で行う（git から見て同じ）
#   - メイン worktree にそのパスが無ければ、作らずに NOTE で知らせる
#   - エラーのときは symlink を 1 つも作らずに止める（worktree は作ったまま残し、場所を知らせる。
#     直すのは .worktreelink か .gitignore で、そのブランチの worktree で直せるように）
#
# GitHub リモートがあり、ローカルにブランチが無く --from も無ければ、ブランチの状態を調べる前に fetch する。古い追跡ブランチから作ると、
# 他のセッションが既定ブランチに入れた変更を取り違える。fetch に失敗したら古い base で進まず止める
# （post-merge-cleanup.sh の fetch と同じ扱い）。
#
# 出力:
#   WORKTREE: <path>
#   BRANCH: <branch>
#   CREATED_BRANCH: yes|no
#   BASE: <ref>（新規作成したときだけ。起点）
#   SHARED: <path>（.worktreelink で symlink にしたパスごと）
#   NOTE: ...（GitHub リモートが複数あるとき、.worktreelink のパスがメイン worktree に無いとき）

set -euo pipefail

usage() {
  echo "Usage: $0 <branch> [--from <ref>] [--carry-changes]" >&2
  exit 1
}

BRANCH=""
CARRY=0
FROM=""
while (( $# > 0 )); do
  case "$1" in
    --carry-changes) CARRY=1 ;;
    --from)
      (( $# >= 2 )) || usage
      FROM="$2"
      shift
      ;;
    -*) usage ;;
    *)
      [[ -z "$BRANCH" ]] || usage
      BRANCH="$1"
      ;;
  esac
  shift
done
[[ -n "$BRANCH" ]] || usage

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "ERROR: git リポジトリではありません" >&2
  exit 1
fi

# メイン worktree（一覧の先頭）を基準にする。linked worktree やサブディレクトリから
# 実行しても同じ場所に作られる
MAIN_ROOT=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
REPO_NAME=$(basename "$MAIN_ROOT")
DIR="$(dirname "$MAIN_ROOT")/${REPO_NAME}-${BRANCH//\//-}"

if [[ -e "$DIR" ]]; then
  echo "ERROR: 作成先が既に存在します: $DIR" >&2
  exit 1
fi

# 既定ブランチ。branch-topics.sh と claude-hooks（git.rs の default_branch）も同じ順で探す
default_branch() {
  local ref
  if ref=$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null); then
    echo "$ref"
    return 0
  fi
  for ref in refs/remotes/origin/main refs/remotes/origin/master refs/heads/main refs/heads/master; do
    if git show-ref --verify --quiet "$ref"; then
      echo "${ref#refs/*/}"
      return 0
    fi
  done
  return 1
}

# base を使うとき（新規作成）か、リモートのブランチを探すときだけ要る。ローカルのブランチか --from なら不要
if [[ -z "$FROM" ]] && ! git show-ref --verify --quiet "refs/heads/$BRANCH" && git remote -v | grep -q 'github\.com'; then
  if ! git fetch -q; then
    echo "ERROR: fetch に失敗しました。古い追跡ブランチから作らないよう止めます。リモートへの接続を確かめてください" >&2
    exit 1
  fi
fi

CREATED=no
BASE=""
REMOTE_REF=""
if ! git show-ref --verify --quiet "refs/heads/$BRANCH"; then
  REMOTE_REF=$(git for-each-ref --format='%(refname:short)' "refs/remotes/*/$BRANCH" | head -n 1)
fi
if git show-ref --verify --quiet "refs/heads/$BRANCH" || [[ -n "$REMOTE_REF" ]]; then
  if [[ -n "$FROM" ]]; then
    echo "ERROR: ブランチ $BRANCH は既にあります。--from は新しいブランチにだけ指定できます" >&2
    exit 1
  fi
  if [[ -n "$REMOTE_REF" ]]; then
    ADD_ARGS=("$DIR" --track -b "$BRANCH" "$REMOTE_REF")
  else
    ADD_ARGS=("$DIR" "$BRANCH")
  fi
else
  if [[ -n "$FROM" ]]; then
    BASE="$FROM"
  elif ! BASE=$(default_branch); then
    echo "ERROR: 既定ブランチ（origin/HEAD, origin/main, origin/master, main, master）が見つかりません。--from <ref> で起点を指定してください" >&2
    exit 1
  fi
  if ! git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null; then
    echo "ERROR: 起点のコミットが見つかりません: $BASE" >&2
    exit 1
  fi
  # 起点がリモート追跡ブランチでも上流にしない（上流が origin/main だと push 先が main になる）
  ADD_ARGS=("$DIR" --no-track -b "$BRANCH" "$BASE")
  CREATED=yes
fi

# stash は worktree 間で共有される（common dir）ので、ここで退避して worktree 側で戻せる
STASHED=0
if (( CARRY )) && [[ -n "$(git status --porcelain)" ]]; then
  git stash push -q -u -m "wip: move to worktree $BRANCH"
  STASHED=1
fi

git worktree add -q "${ADD_ARGS[@]}"

if (( STASHED )); then
  if ! git -C "$DIR" stash pop -q --index; then
    echo "ERROR: 変更の適用に失敗しました。変更は stash に残っています（git stash list）" >&2
    exit 1
  fi
fi

echo "WORKTREE: $DIR"
echo "BRANCH: $BRANCH"
echo "CREATED_BRANCH: $CREATED"
[[ -z "$BASE" ]] || echo "BASE: $BASE"

# fork 運用（GitHub リモートが複数）では gh の解決先が worktree に引き継がれないことがある
GH_REMOTES=$(git remote -v | awk '/github\.com/ && /\(fetch\)/ { print $1 }' | sort -u | wc -l)
if (( GH_REMOTES >= 2 )); then
  echo "NOTE: GitHub リモートが複数あります。worktree で gh pr 系コマンドを使う前に gh repo set-default --view で解決先を確認してください"
fi

# --- .worktreelink: メイン worktree のパスを symlink で共有する ---
# 設定はチェックアウトしたブランチのもの（ブランチごとに .gitignore と揃っている）を読む
LINK_FILE="$DIR/.worktreelink"
if [[ -f "$LINK_FILE" ]]; then
  SHARE_PATHS=()
  SHARE_ERRORS=()
  SEEN_PATHS=()
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ "$line" =~ ^[[:space:]]*$ || "$line" == \#* ]] && continue
    # 空白は名前の一部になりうるので、落とさずに止める
    if [[ "$line" =~ ^[[:space:]] || "$line" =~ [[:space:]]$ ]]; then
      SHARE_ERRORS+=("前後に空白があります: '$line'")
      continue
    fi
    # 末尾の / は symlink の名前には要らない（references/ と references は同じ）
    while [[ "$line" == */ ]]; do line="${line%/}"; done
    # git は引数を pathspec として読む（check-ignore は GIT_LITERAL_PATHSPECS を受け付けない）ので、
    # 魔法（:foo、:(top)foo）、ワイルドカード、エスケープになる文字を止め、symlink の名前そのものを判定させる
    if [[ -z "$line" || "$line" == /* || "$line" == !* || "$line" == :* || "$line" == *[*?[\\]* ]]; then
      SHARE_ERRORS+=("リテラルの相対パスではありません: $line")
      continue
    fi
    if [[ "/$line/" == */./* || "/$line/" == */../* || "/$line/" == *//* ]]; then
      SHARE_ERRORS+=(". / .. や空の要素を含むパスは使えません: $line")
      continue
    fi
    # 同じパスや入れ子のパスは、後の ln -s が先の symlink をたどってメイン worktree の中に作ってしまう
    overlap=0
    for seen in "${SEEN_PATHS[@]}"; do
      if [[ "$line" == "$seen" || "$line" == "$seen"/* || "$seen" == "$line"/* ]]; then
        overlap=1
        break
      fi
    done
    SEEN_PATHS+=("$line")
    if (( overlap )); then
      SHARE_ERRORS+=("重なるパスがあります: $line")
      continue
    fi
    # 親は無いか、symlink でないディレクトリであること（mkdir -p と ln -s が途中で落ちたり、
    # symlink をたどってメイン worktree の中に作ったりしないように）
    parent_ok=1
    ancestor="$DIR"
    IFS=/ read -ra parts <<< "$line"
    for part in "${parts[@]:0:${#parts[@]}-1}"; do
      ancestor="$ancestor/$part"
      if [[ -L "$ancestor" || ( -e "$ancestor" && ! -d "$ancestor" ) ]]; then
        parent_ok=0
        break
      fi
    done
    if (( ! parent_ok )); then
      SHARE_ERRORS+=("親が新しい worktree でディレクトリではありません: $line")
      continue
    fi
    if [[ -e "$DIR/$line" || -L "$DIR/$line" ]]; then
      SHARE_ERRORS+=("新しい worktree に既にあります（追跡されているか、--carry-changes で持ち込んだ）: $line")
      continue
    fi
    rc=0
    ignore_err=$(git -C "$DIR" check-ignore -q -- "$line" 2>&1) || rc=$?
    if (( rc == 1 )); then
      SHARE_ERRORS+=("git に無視されていません（symlink が未追跡ファイルに見える。dir/ でなく dir か /dir と書く）: $line")
      continue
    elif (( rc != 0 )); then
      SHARE_ERRORS+=("git check-ignore が失敗しました（終了コード $rc: $ignore_err）: $line")
      continue
    fi
    # メインのブランチが追跡するパスに張ると、新しい worktree での書き込みがメインの変更になる
    if [[ -n "$(git -C "$MAIN_ROOT" ls-files -- "$line")" ]]; then
      SHARE_ERRORS+=("メイン worktree で git に追跡されています: $line")
      continue
    fi
    SHARE_PATHS+=("$line")
  done < "$LINK_FILE"

  if (( ${#SHARE_ERRORS[@]} > 0 )); then
    {
      echo "ERROR: .worktreelink に使えないパスがあります。symlink は作っていません:"
      printf '  %s\n' "${SHARE_ERRORS[@]}"
      echo "worktree は $DIR に作ってあります。そのブランチで .worktreelink か .gitignore を直してください"
    } >&2
    exit 1
  fi

  # 途中で mkdir / ln が失敗したら、作った symlink を消してから止める（1 つも残さない）
  LINKED=()
  NOTES=()
  for path in "${SHARE_PATHS[@]}"; do
    if [[ ! -e "$MAIN_ROOT/$path" ]]; then
      NOTES+=("NOTE: メイン worktree に $path がないため共有しません（$MAIN_ROOT/$path）")
      continue
    fi
    if ! mkdir -p "$(dirname "$DIR/$path")" || ! ln -s "$MAIN_ROOT/$path" "$DIR/$path"; then
      for linked in "${LINKED[@]}"; do rm -f "$DIR/$linked"; done
      echo "ERROR: $path の symlink を作れませんでした。作った symlink は消しました。worktree は $DIR に作ってあります" >&2
      exit 1
    fi
    LINKED+=("$path")
  done
  for note in "${NOTES[@]}"; do echo "$note"; done
  for linked in "${LINKED[@]}"; do echo "SHARED: $linked"; done
fi

