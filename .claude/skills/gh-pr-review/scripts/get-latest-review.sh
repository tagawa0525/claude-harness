#!/usr/bin/env bash
# gh-pr-review: 最新の Copilot レビューの要約を取得するスクリプト
#
# Usage: get-latest-review.sh <PR の URL>
#
# 出力（テキスト）:
#   ROUND: <n>              PR 上の Copilot レビュー件数（周回数そのものではない。
#                           decide-next.sh がレビュー要求件数と突き合わせて周回を決める）
#   REVIEW_ID: <id>
#   STATE: <state>
#   HEADLINE: <本文1行目>    例: "### 🟢 Approval recommended"
#   REVIEW_FAILED: yes|no   Copilot がレビューできずに終わったレビューか
#   SUCCESSFUL_REVIEWS: <n> 失敗していない Copilot レビューの件数（文書だけの PR は、
#                           一度でも成功していれば後のレビューが失敗しても止めない）
#   INLINE_COMMENTS: <n>    このレビューに紐づくインラインコメント数（通常の指摘）
#   SUPPRESSED_COMMENTS: <n>
#   SEVERITY: v2|unknown    本文が重大度を持つ形（v2）か。unknown なら REMAINING_* は出さない
#   REMAINING_HIGH: <n>     残っている High（開いている指摘と Previously missed）
#   REMAINING_MEDIUM: <n>
#   REMAINING_LOW: <n>
#   PREVIOUSLY_MISSED: <n>  Previously missed の指摘の数（v2 の本文なら、重大度が分からなくても出す）
#   --- suppressed ---      以降、Suppressed comments セクションの本文
#   --- previously missed --- 以降、Previously missed の節の本文
#
# Suppressed comments は Copilot が低確度と判断した指摘で、インラインスレッドには
# ならずレビュー本文の <details> 内にだけ現れる。get-review-comments.sh では
# 取得できないため、このスクリプトで別途読む。
#
# Copilot はレビューできなかったときも本文だけのレビューを提出する
# （"Copilot wasn't able to review any files in this pull request." など）。
# インライン指摘は 0 件なので、区別しないと「指摘なし」と同じ扱いになる。

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/pr-url.sh"
if ! parse_pr_url "${1:-}"; then
  echo "Usage: $0 <PR の URL>" >&2
  exit 1
fi

# Copilot のレビューを提出順に全件取得（周回数の算出に全件必要）。
# --paginate に配列を返す --jq を渡すとページごとの配列が連結されて不正な JSON に
# なるため、要素をストリーム出力してから jq -s で1つの配列にまとめる
reviews=$(gh api --paginate "repos/{owner}/{repo}/pulls/${PR_NUMBER}/reviews?per_page=100" \
  --jq '.[] | select(.user.login == "copilot-pull-request-reviewer[bot]")' \
  | jq -s '.')

round=$(jq 'length' <<<"$reviews")
if (( round == 0 )); then
  echo "ROUND: 0"
  echo "NOTE: Copilot のレビューはまだありません"
  exit 0
fi

latest=$(jq '.[-1]' <<<"$reviews")
review_id=$(jq -r '.id' <<<"$latest")
state=$(jq -r '.state' <<<"$latest")
body=$(jq -r '.body' <<<"$latest")

inline_count=$(gh api --paginate \
  "repos/{owner}/{repo}/pulls/${PR_NUMBER}/reviews/${review_id}/comments?per_page=100" \
  --jq '.[].id' | wc -l)

# "Suppressed comments (N)" の行から次の </details> までを抜き出す。
# 見出しは <summary> タグに包まれる形式もありうるため行頭に固定しない
suppressed=$(awk '
  /Suppressed comments \([0-9]+\)/ { on=1 }
  on && (/<\/details>/ || /^- \*\*Files reviewed:/) { exit }
  on { print }
' <<<"$body")
suppressed_count=$(grep -o -E 'Suppressed comments \([0-9]+\)' <<<"$suppressed" \
  | head -n 1 | grep -o -E '[0-9]+' || echo 0)

# 重大度。2026-09-19 からの本文（<!-- ccr-overview-v2 -->）は、指摘ごとに重大度のアイコン
# （alt="High severity" など）を持つ。節は見出しの行（<summary><strong>…）で分かれる。"N open findings"
# （2026-09 の本文は "Open (N)"）は
# 開いている指摘（その回の新しい指摘と、前の周から残る指摘。"- " で始まる行）、"Previously missed" は
# 前の周で見落とした、変わっていないコードへの指摘（入れ子の <summary> の行。本文にだけあってスレッドに
# ならない）。ほかの節（解決した指摘、"What changed in this PR" の変更の一覧など）は数えない。節は外側の
# </details> で閉じる（Previously missed の中の入れ子の <details> は深さで数える）。知らない見出しの節や、
# 開いている指摘の節の「- 」で始まらない行、Previously missed の節の <summary> でない行のアイコン、知っている節の
# 外のアイコン（要約の **Findings:** の行は除く）があれば、形が変わって High を数え落としうるので、分からないとする。節の切り替えを見出しの行だけで行うのは、指摘の
# 題名に同じ語が入りうるため。アイコンの無い指摘があれば、形が変わって数え落としうるので、重大度は
# 分からない（unknown）とする
# mode=items は「節 重大度」（重大度が無ければ none）を 1 行 1 指摘で出し、形の変化の印（指摘でない行のアイコンなど）は
# 「<場所> none」で出す（件数には数えない）。mode=missed は Previously missed の節の行を出す
SECTIONS_AWK='
  BEGIN { sec = "none"; depth = 0 }
  /<summary><strong>/ {
    title = tolower($0)
    sub(/.*<summary><strong>[0-9 ]*/, "", title)
    if (title ~ /^open/) sec = "open"
    else if (title ~ /^previously missed/) sec = "missed"
    else if (title ~ /^(resolved since last review|what changed in this pr)/) sec = "known"
    else { sec = "known"; print "heading none" }
    depth = 1
    next
  }
  mode == "items" && sec == "open" && /[^[:space:]]/ && !/^[[:space:]]*- / && !/<\/details>/ { print "open none" }
  mode == "items" && ((sec == "open" && /^[[:space:]]*- /) || (sec == "missed" && /<summary>/)) {
    # 1 行に複数のアイコンがあれば High を優先する（pre_merge_check.rs と同じ）
    if (/alt="High severity"/) print sec, "High"
    else if (/alt="Medium severity"/) print sec, "Medium"
    else if (/alt="Low severity"/) print sec, "Low"
    else print sec, "none"
  }
  mode == "items" && sec == "missed" && !/<summary>/ && /alt="[^"]* severity"/ { print "missed-line none" }
  mode == "items" && sec == "none" && /alt="[^"]* severity"/ && !/\*\*Findings:\*\*/ { print "outside none" }
  mode == "missed" && sec == "missed" { print }
  /<details/ { if (sec != "none") depth++ }
  /<\/details>/ { if (sec != "none" && --depth == 0) sec = "none" }
  # 節が閉じないまま終わる本文（切れた本文）は、残りの指摘を数え落としうる
  END { if (mode == "items" && sec != "none") print "eof none" }
'
VERDICT_RE='^### .*(Approval recommended|Changes recommended|Needs a closer look)'
severity=unknown
items=""
missed_part=""
if grep -qF '<!-- ccr-overview-v2 -->' <<<"$body"; then
  items=$(awk -v mode=items "$SECTIONS_AWK" <<<"$body")
  # Previously missed はスレッドが無く本文にしか無いので、重大度が分からなくても数えて見せる
  missed_part=$(awk -v mode=missed "$SECTIONS_AWK" <<<"$body")
  # 判定の見出しが無い本文（切れた本文、完走しなかったレビュー）は指摘を数え落としうる
  if ! grep -q ' none$' <<<"$items" && grep -qE "$VERDICT_RE" <<<"$body"; then
    severity=v2
  fi
fi
# count_items <正規表現>: items のうち一致する行の数（見つからないときの grep の 1 で、pipefail と
# set -e が呼び出し元の代入ごと止めないようにする）
count_items() {
  { grep -c -E "$1" <<<"$items" || true; }
}

# レビュー失敗は本文1行目に出る（インライン指摘もサマリーも無い）
failed_pattern="(wasn'?t able to review|unable to review|encountered an error)"
review_failed=no
if head -n 1 <<<"$body" | grep -q -i -E "$failed_pattern"; then
  review_failed=yes
fi
successful_reviews=0
while IFS= read -r first_line; do
  grep -q -i -E "$failed_pattern" <<<"$first_line" || successful_reviews=$((successful_reviews + 1))
done < <(jq -r '.[] | (.body // "") | split("\n")[0]' <<<"$reviews")

echo "ROUND: ${round}"
echo "REVIEW_ID: ${review_id}"
echo "STATE: ${state}"
echo "HEADLINE: $(head -n 1 <<<"$body")"
echo "REVIEW_FAILED: ${review_failed}"
echo "SUCCESSFUL_REVIEWS: ${successful_reviews}"
echo "INLINE_COMMENTS: ${inline_count}"
echo "SUPPRESSED_COMMENTS: ${suppressed_count}"
echo "SEVERITY: ${severity}"
if [[ "$severity" == v2 ]]; then
  for level in High Medium Low; do
    echo "REMAINING_${level^^}: $(count_items " ${level}\$")"
  done
fi
missed_count=0
if grep -qF '<!-- ccr-overview-v2 -->' <<<"$body"; then
  missed_count=$(count_items '^missed ')
  echo "PREVIOUSLY_MISSED: ${missed_count}"
fi
if (( suppressed_count > 0 )); then
  echo "--- suppressed ---"
  echo "$suppressed"
fi
if (( missed_count > 0 )); then
  echo "--- previously missed ---"
  echo "$missed_part"
fi
