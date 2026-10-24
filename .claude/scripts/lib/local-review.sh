# shellcheck shell=bash
# local-review.sh — ローカルレビューの記録（record-local-review.sh が PR に付けるコメント）を読む（source して使う）
#
# local_review_records
#   PR_NUMBER の PR に付いたローカルレビューの記録を、古い順に「SHA High の数」の 1 行ずつ出力する（parse_pr_url の
#   後に呼ぶ）。取得に失敗したら 1 を返す
#
# local_review_clean_shas
#   標準入力の local_review_records の出力のうち、High の無いもの（high=0）の SHA を、1 行 1 つで出力する。High を直せば head が変わるので、High のある記録の head には直して
#   いない High が残る。同じ SHA に記録が複数あれば、最後の記録で決める（回し直して見つけた High を、前の
#   記録で隠さない）。
#   記録はリポジトリに書き込める人のコメントだけを数える（PR にコメントできる第三者が記録を偽って
#   記録でマージの検査を通せないように。reset-rounds.sh の印と同じ）。
#   形は pre-merge-check（claude-hooks の pre_merge_check.rs）も読む。記録として読むのはコメントの 1 行目だけで、
#   `<!-- local-review ` で始まるのに形の崩れた行も記録に数える（High ありと同じく通さない。pre_merge_check.rs と同じ）
#
# local_review_covers <head の SHA>
#   head がローカルレビューの記録で覆われていれば yes、そうでなければ no を出力する。head の記録（High なし）があるか、
#   最後の記録の SHA から rebase だけした head なら yes。最後の記録は SHA を問わず古い順の最後のもので、それに High が
#   あれば比べない（High の無い記録のうち最後のものを選ぶと、同じ差分を回し直して見つけた High を前の記録で隠す）。
#   rebase だけかは pre-merge-check と同じ実装（claude-hooks same-pr-diff）で確かめ、bash で書き直さない。比べるコミットは
#   cwd の clone から取るので、cwd の origin が PR のリポジトリでなければ比べずに no（止める側。無関係なリポジトリで
#   git fetch しない）。取得や比較に失敗したら 1 を返す

# shellcheck disable=SC2034  # record-local-review.sh が使う
LOCAL_REVIEW_MARK='<!-- local-review '

# 古い順に「SHA High の数」を 1 行ずつ。形の崩れた記録は「- -」
local_review_records() {
  gh api --paginate "repos/{owner}/{repo}/issues/${PR_NUMBER}/comments?per_page=100" \
    --jq '.[] | select(.author_association | IN("OWNER", "MEMBER", "COLLABORATOR")) | (.body | split("\n")[0]) | select(startswith("<!-- local-review ")) | ((capture("^<!-- local-review sha=(?<sha>[0-9a-f]+) high=(?<high>[0-9]+) ") | "\(.sha) \(.high)") // "- -")'
}

local_review_clean_shas() {
  awk '!($1 in high) { order[++n] = $1 } { high[$1] = $2 } END { for (i = 1; i <= n; i++) if (high[order[i]] == "0") print order[i] }'
}

local_review_covers() {
  local head="$1" records shas source origin base_ref hooks_bin rc=0
  records=$(local_review_records) || return 1
  shas=$(local_review_clean_shas <<<"$records")
  if grep -qxF "$head" <<<"$shas"; then
    echo yes
    return 0
  fi
  source=$(awk 'NF { last = $0 } END { split(last, f, " "); if (f[2] == "0") print f[1] }' <<<"$records")
  origin=$(git remote get-url origin 2>/dev/null) || origin=""
  origin=${origin%.git}
  if [[ -z "$source" || "$source" == "$head" || "$origin" != *github.com[:/]"$GH_REPO" ]]; then
    echo no
    return 0
  fi
  if ! base_ref=$(gh pr view "$PR_NUMBER" --json baseRefName -q .baseRefName) || [[ -z "$base_ref" ]]; then
    echo "ERROR: PR #${PR_NUMBER} の base ブランチを取得できませんでした" >&2
    return 1
  fi
  # ~/.claude/bin は claude-code.nix が置く。スクリプトの場所から辿らないのは、リポジトリの checkout から実行しても同じ
  # バイナリを使うため
  hooks_bin="${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}"
  "$hooks_bin" same-pr-diff "$GH_REPO" "$base_ref" "$source" "$head" || rc=$?
  case "$rc" in
    0) echo yes ;;
    1) echo no ;;
    *)
      echo "ERROR: 記録 ${source:0:7} と head ${head:0:7} の差分を比べられませんでした（${hooks_bin} の終了コード ${rc}）" >&2
      return 1
      ;;
  esac
}
