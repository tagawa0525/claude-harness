#!/usr/bin/env bash
# prose-size.sh — 区間の前後の文章（メモリ、スキル、CLAUDE.md）の大きさをバイトで出す
#
# Usage: prose-size.sh [--since <日時>] [--until <日時>] [--memory <メモリのディレクトリ>] [--repo <パス>]...
#   --since, --until
#               区間 [since, until)。日時は ISO 8601 で、時差（Z か +09:00 の形）を必ず付ける（シェルの TZ で意味が
#               変わらないため）。--since を省くと JST の今日の 0 時
#   --memory    自動メモリのディレクトリ（~/.claude/projects/<プロジェクト>/memory）。前の大きさは ~/.claude の git の
#               refs/memory-backup の、since より前の最後の退避で数える。後の大きさは、--until があれば until より
#               前の最後の退避、無ければ今のディスクのファイルで数える（区間の終わりの退避より後に書いたメモリも入れる）
#   --repo      リポジトリか、その worktree。前の大きさは既定ブランチ（origin/HEAD、無ければ main か master）の
#               first-parent で since より前の最後のコミットで数える。後の大きさは、--until があれば同じ既定
#               ブランチの until より前の最後のコミット、無ければ既定ブランチの今の大きさに、渡したパスの HEAD が
#               既定ブランチとの分岐点から変えた分を足す（worktree を渡せばまだマージしていないブランチの分も入り、
#               遅れた checkout を渡しても既定ブランチにマージした分は落ちない。両方が同じファイルを変えていれば、
#               HEAD の側の変更を既定ブランチの変更に重ねて数える）。作業ツリーの未コミットの変更は数えない
#
# 数える範囲（機械の hook と script は数えない）:
#   memory     メモリのディレクトリの下のすべてのファイル（退避と同じ範囲）
#   skills     リポジトリの .claude/skills/ の下の *.md
#   claude-md  リポジトリのどこかにある CLAUDE.md
#
# 出力:
#   SINCE: <JST の日時>
#   UNTIL: <JST の日時 | now>
#   PROSE: <memory|skills|claude-md> <前> <後> <差（+N、-N、0）> <渡したパス>
#
# メモリのディレクトリも退避も無ければ（メモリの無いプロジェクト）0 と数える。メモリのディレクトリか退避の中身が
# あるのに since より前の退避が無いとき（退避を始める前の区間）と、~/.claude の外のディレクトリを渡したときは、
# 前の大きさが分からないので、ほかの行を出した後に ERROR: を標準エラーに出して exit 1（0 と出すと、増えた分を
# 区間の増分と読む）。

set -euo pipefail

usage() {
  echo "Usage: prose-size.sh [--since <日時>] [--until <日時>] [--memory <メモリのディレクトリ>] [--repo <パス>]..." >&2
  exit 1
}

# to_epoch <オプション名> <日時>: 時差つきの ISO 8601 を UNIX 時刻にする
to_epoch() {
  local re='^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}(:[0-9]{2}(\.[0-9]+)?)?(Z|[+-][0-9]{2}:[0-9]{2})$'
  if [[ ! "$2" =~ $re ]]; then
    echo "ERROR: $1 は時差つきの ISO 8601 で書いてください（例 2026-11-06T00:00+09:00、2026-11-05T15:00Z）: $2" >&2
    exit 1
  fi
  date -d "$2" +%s 2>/dev/null || {
    echo "ERROR: $1 の日時を読めません: $2" >&2
    exit 1
  }
}

# jst <UNIX 時刻>: JST の ISO 8601。tzdata の無い環境でも同じ結果にするため、TZ でなく 9 時間を足して書く
jst() {
  date -u -d "@$(($1 + 9 * 3600))" +%Y-%m-%dT%H:%M:%S+09:00
}

since=""
until=""
memory=""
repos=()
while (($#)); do
  case "$1" in
    --since | --until | --memory | --repo)
      (($# >= 2)) || usage
      case "$1" in
        --since) since=$(to_epoch --since "$2") ;;
        --until) until=$(to_epoch --until "$2") ;;
        --memory) memory=${2%/} ;;
        --repo) repos+=("$2") ;;
      esac
      shift 2
      ;;
    *) usage ;;
  esac
done
[[ -n "$memory" || ${#repos[@]} -gt 0 ]] || usage
if [[ -z "$since" ]]; then
  now=$(date +%s)
  since=$(date -d "$(date -u -d "@$((now + 9 * 3600))" +%Y-%m-%d)T00:00:00+09:00" +%s)
fi
if [[ -n "$until" ]] && ((since >= until)); then
  echo "ERROR: --since（$(jst "$since")）は --until（$(jst "$until")）より前にしてください" >&2
  exit 1
fi

echo "SINCE: $(jst "$since")"
if [[ -n "$until" ]]; then echo "UNTIL: $(jst "$until")"; else echo "UNTIL: now"; fi

# git の --before は指定の時刻ちょうどのコミットを含むので、1 秒前を渡して区間 [since, until) の外に置く
before_arg() {
  echo "@$(($1 - 1)) +0000"
}

# tree_bytes <git -C の引数> <コミット> <awk のパスの条件>: コミットの中で条件に合うファイルの大きさの和。
# コミットが空なら 0。条件の中のパスの接頭辞は ENVIRON["PREFIX"] で渡す（awk のプログラムに埋めると、引用符や
# 逆斜線を含むパスでプログラムが変わる）
tree_bytes() {
  local dir=$1 commit=$2 cond=$3
  [[ -n "$commit" ]] || {
    echo 0
    return
  }
  git -C "$dir" ls-tree -r -l -z "$commit" |
    awk -v RS='\0' "{ split(\$0, a, \"\\t\"); split(a[1], m, \" \"); path = a[2] } m[2] == \"blob\" && ($cond) { s += m[4] } END { print s + 0 }"
}

# repo_bytes <repo> <awk のパスの条件>: 後の大きさ。--until が無ければ、既定ブランチの今に、HEAD が分岐点から
# 変えた分を足す（base + HEAD - 分岐点）
repo_bytes() {
  local repo=$1 cond=$2
  if [[ -n "$until" ]]; then
    tree_bytes "$repo" "$new" "$cond"
    return
  fi
  echo $(($(tree_bytes "$repo" "$tip" "$cond") + $(tree_bytes "$repo" "$head" "$cond") - $(tree_bytes "$repo" "$fork" "$cond")))
}

delta() {
  local d=$(($2 - $1))
  if ((d > 0)); then echo "+$d"; else echo "$d"; fi
}

failed=0

for repo in "${repos[@]}"; do
  base=""
  for ref in origin/HEAD main master; do
    if git -C "$repo" rev-parse -q --verify "$ref^{commit}" >/dev/null; then
      base=$ref
      break
    fi
  done
  if [[ -z "$base" ]]; then
    echo "ERROR: $repo に既定ブランチ（origin/HEAD、main、master）がありません" >&2
    failed=1
    continue
  fi
  old=$(git -C "$repo" rev-list -1 --first-parent --before="$(before_arg "$since")" "$base")
  if [[ -n "$until" ]]; then
    new=$(git -C "$repo" rev-list -1 --first-parent --before="$(before_arg "$until")" "$base")
  else
    tip=$(git -C "$repo" rev-parse "$base")
    head=$(git -C "$repo" rev-parse HEAD)
    fork=$(git -C "$repo" merge-base "$tip" "$head")
  fi
  skills='path ~ /^\.claude\/skills\/.*\.md$/'
  claude_md='path == "CLAUDE.md" || path ~ /\/CLAUDE\.md$/'
  a=$(tree_bytes "$repo" "$old" "$skills")
  b=$(repo_bytes "$repo" "$skills")
  echo "PROSE: skills $a $b $(delta "$a" "$b") $repo"
  a=$(tree_bytes "$repo" "$old" "$claude_md")
  b=$(repo_bytes "$repo" "$claude_md")
  echo "PROSE: claude-md $a $b $(delta "$a" "$b") $repo"
done

if [[ -n "$memory" ]]; then
  home="$HOME/.claude"
  rel=${memory#"$home"/}
  if [[ "$rel" == "$memory" ]]; then
    echo "ERROR: メモリのディレクトリが $home の下にありません（退避は $home の git にあります）: $memory" >&2
    exit 1
  fi
  ref=refs/memory-backup
  has_ref=0
  if git -C "$home" rev-parse -q --verify "$ref^{commit}" >/dev/null 2>&1; then
    has_ref=1
  fi
  # 退避の中身にこのディレクトリがあるか（退避を始める前の区間を、メモリの無いプロジェクトと見分ける）
  in_backup=0
  if ((has_ref)) && [[ -n "$(git -C "$home" ls-tree -r --name-only "$ref" -- ":(literal)$rel/")" ]]; then
    in_backup=1
  fi
  old_commit=""
  ((has_ref)) && old_commit=$(git -C "$home" rev-list -1 --before="$(before_arg "$since")" "$ref")
  if [[ -z "$old_commit" ]] && { [[ -d "$memory" ]] || ((in_backup)); }; then
    echo "ERROR: $(jst "$since") より前のメモリの退避（$home の $ref）が無いので、前の大きさが分かりません: $memory" >&2
    exit 1
  fi
  export PREFIX="$rel/"
  cond='index(path, ENVIRON["PREFIX"]) == 1'
  a=$(tree_bytes "$home" "$old_commit" "$cond")
  if [[ -n "$until" ]]; then
    new_commit=""
    ((has_ref)) && new_commit=$(git -C "$home" rev-list -1 --before="$(before_arg "$until")" "$ref")
    b=$(tree_bytes "$home" "$new_commit" "$cond")
  elif [[ -d "$memory" ]]; then
    b=$(find "$memory" -type f -printf '%s\n' | awk '{ s += $1 } END { print s + 0 }')
  else
    b=0
  fi
  echo "PROSE: memory $a $b $(delta "$a" "$b") $memory"
fi

exit "$failed"
