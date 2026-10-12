# shellcheck shell=bash
# local-review.sh — ローカルレビューの記録（record-local-review.sh が PR に付けるコメント）を読む（source して使う）
#
# local_review_shas
#   PR_NUMBER の PR に付いたローカルレビューの記録の SHA を、1 行 1 つで出力する（parse_pr_url の後に呼ぶ）。
#   記録はリポジトリに書き込める人のコメントだけを数える（PR にコメントできる第三者が記録を偽って
#   Copilot の再レビューを外せないように。reset-rounds.sh の印と同じ）。取得に失敗したら 1 を返す。
#   形は pre-merge-check（claude-hooks の pre_merge_check.rs）も読む（ADR-0034）

LOCAL_REVIEW_MARK='<!-- local-review '

local_review_shas() {
  gh api --paginate "repos/{owner}/{repo}/issues/${PR_NUMBER}/comments?per_page=100" \
    --jq '.[] | select(.author_association | IN("OWNER", "MEMBER", "COLLABORATOR")) | .body | select(startswith("<!-- local-review ")) | capture("^<!-- local-review sha=(?<sha>[0-9a-f]+) ").sha'
}
