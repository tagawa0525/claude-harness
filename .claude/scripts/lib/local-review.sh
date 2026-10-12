# shellcheck shell=bash
# local-review.sh — ローカルレビューの記録（record-local-review.sh が PR に付けるコメント）を読む（source して使う）
#
# local_review_shas
#   PR_NUMBER の PR に付いたローカルレビューの記録のうち、High の無いもの（high=0）の SHA を、1 行 1 つで
#   出力する（parse_pr_url の後に呼ぶ）。High を直せば head が変わるので、High のある記録の head には直して
#   いない High が残る。
#   記録はリポジトリに書き込める人のコメントだけを数える（PR にコメントできる第三者が記録を偽って
#   Copilot の再レビューを外せないように。reset-rounds.sh の印と同じ）。取得に失敗したら 1 を返す。
#   形は pre-merge-check（claude-hooks の pre_merge_check.rs）も読む（ADR-0034）

# shellcheck disable=SC2034  # record-local-review.sh が使う
LOCAL_REVIEW_MARK='<!-- local-review '

local_review_shas() {
  gh api --paginate "repos/{owner}/{repo}/issues/${PR_NUMBER}/comments?per_page=100" \
    --jq '.[] | select(.author_association | IN("OWNER", "MEMBER", "COLLABORATOR")) | .body | select(startswith("<!-- local-review ")) | capture("^<!-- local-review sha=(?<sha>[0-9a-f]+) high=0 ").sha'
}
