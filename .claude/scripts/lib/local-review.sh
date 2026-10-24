# shellcheck shell=bash
# local-review.sh — ローカルレビューの記録（record-local-review.sh が PR に付けるコメント）を読む（source して使う）
#
# local_review_shas
#   PR_NUMBER の PR に付いたローカルレビューの記録のうち、High の無いもの（high=0）の SHA を、1 行 1 つで
#   出力する（parse_pr_url の後に呼ぶ）。High を直せば head が変わるので、High のある記録の head には直して
#   いない High が残る。同じ SHA に記録が複数あれば、最後の記録で決める（回し直して見つけた High を、前の
#   記録で隠さない）。
#   記録はリポジトリに書き込める人のコメントだけを数える（PR にコメントできる第三者が記録を偽って
#   記録でマージの検査を通せないように。reset-rounds.sh の印と同じ）。取得に失敗したら 1 を返す。
#   形は pre-merge-check（claude-hooks の pre_merge_check.rs）も読む
#
# local_review_records
#   同じ記録を古い順に「SHA High の数」の 1 行ずつ出力する（High の有無で絞らない）。取得に失敗したら 1 を返す
#
# local_review_clean_shas
#   標準入力の local_review_records の出力から、local_review_shas と同じ SHA を出力する（取得を 1 回で済ませる呼び出し側用）
#
# local_review_rebase_source
#   標準入力の local_review_records の出力から、リベースだけの判定の比較元にする SHA を出力する。SHA を問わず最後の
#   記録を使い、それに High があれば何も出力しない（High の無い記録のうち最後のものを選ぶと、同じ差分を回し直して
#   見つけた High を前の記録で隠す）。選び方は pre_merge_check.rs の local_review_rebase_source と揃える

# shellcheck disable=SC2034  # record-local-review.sh が使う
LOCAL_REVIEW_MARK='<!-- local-review '

local_review_records() {
  gh api --paginate "repos/{owner}/{repo}/issues/${PR_NUMBER}/comments?per_page=100" \
    --jq '.[] | select(.author_association | IN("OWNER", "MEMBER", "COLLABORATOR")) | .body | capture("^<!-- local-review sha=(?<sha>[0-9a-f]+) high=(?<high>[0-9]+) ") | "\(.sha) \(.high)"'
}

local_review_rebase_source() {
  awk 'NF { last = $0 } END { split(last, f, " "); if (f[2] == "0") print f[1] }'
}

local_review_shas() {
  local records
  records=$(local_review_records) || return 1
  local_review_clean_shas <<<"$records"
}

local_review_clean_shas() {
  awk '!($1 in high) { order[++n] = $1 } { high[$1] = $2 } END { for (i = 1; i <= n; i++) if (high[order[i]] == "0") print order[i] }'
}
