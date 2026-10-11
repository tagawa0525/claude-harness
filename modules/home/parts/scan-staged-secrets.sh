# 機密情報の検査（ADR-0015）。Claude Code のセッション（CLAUDECODE=1）のコミットで、コミットに入る中身（index と
# HEAD の差）を gitleaks で見る。グローバルの pre-commit と pre-merge-commit が、index を変える検査の後に呼ぶ。
# 値は出さず、場所とルール ID だけを出す。漏えいと、検査できないこと（gitleaks の失敗）を分けて出し、どちらも
# 終了コード 1 で止める。迂回は ALLOW_SECRET_COMMIT=1
#
# 検証: modules/home/parts/tests/pre-commit-secrets.sh

[ "${CLAUDECODE:-}" = "1" ] || exit 0
[ "${ALLOW_SECRET_COMMIT:-}" != "1" ] || exit 0

err=$(mktemp)
trap 'rm -f "$err"' EXIT

rc=0
# --log-level error: gitleaks は中の git diff の失敗を ERR に出すだけで、終了コード 0（漏えいなし）を返すので、
# エラーの行が出たら検査できなかったとみなす
leaks=$(gitleaks git --staged --no-banner --redact --log-level error \
  --report-format json --report-path - . 2>"$err") || rc=$?

# gitleaks は漏えいでも、設定を読めないなどの失敗でも 1 を返すので、一覧が空でないときだけ漏えいとする
count=0
if [ -n "$leaks" ]; then
  count=$(printf '%s' "$leaks" | jq 'length' 2>/dev/null) || count=0
fi
if [ "$rc" -eq 1 ] && [ "${count:-0}" -gt 0 ]; then
  echo "❌ 機密情報らしき内容がコミット対象に含まれています（gitleaks）:"
  printf '%s\n' "$leaks" | jq -r '.[] | "- \(.File):\(.StartLine) (\(.RuleID))"'
  echo "   対処: git restore --staged <file> で外し、必要なら .gitignore に足してください。push 済みの値はローテーションしてください"
  echo "   誤検出なら、その行に gitleaks:allow を書く（.gitleaksignore / .gitleaks.toml でも可）か、コマンドに ALLOW_SECRET_COMMIT=1 を付けてください"
  exit 1
fi
if [ "$rc" -ne 0 ] || [ -s "$err" ]; then
  echo "❌ gitleaks が失敗しました（終了コード $rc）。機密情報を検査できないので、コミットを止めます:"
  sed 's/^/   /' "$err"
  exit 1
fi
