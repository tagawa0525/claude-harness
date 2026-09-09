#!/usr/bin/env bash
# gh-review-requests.sh — PR の Copilot 宛てレビュー要求の時刻を発生順に出す
#
# Usage: gh-review-requests.sh <pr_number>
#
# 出力: review_requested イベントの created_at（ISO 8601）を 1 行ずつ、古い順。
# 要求が無ければ何も出さずに終了コード 0。取得に失敗したら 1。
#
# レビュアーのログイン表記は API により Copilot / copilot-pull-request-reviewer[bot] と
# 揺れるため、Bot 種別かつ login に copilot を含むもので判定する。
#
# 「レビューを待つ」「何周目か」「再レビューが登録されたか」はいずれもこの列から
# 決まるので、取得を 1 か所にまとめている。

set -euo pipefail

PR_NUMBER="${1:-}"
if [[ -z "$PR_NUMBER" ]]; then
  echo "Usage: $0 <pr_number>" >&2
  exit 1
fi

gh api --paginate "repos/{owner}/{repo}/issues/${PR_NUMBER}/timeline?per_page=100" \
  --jq '.[] | select(.event == "review_requested"
                     and (.requested_reviewer.type? // "") == "Bot"
                     and ((.requested_reviewer.login? // "") | ascii_downcase | test("copilot")))
           | .created_at'
