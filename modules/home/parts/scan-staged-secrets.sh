# 機密情報の検査（ADR-0015）。Claude Code のセッション（CLAUDECODE=1）のコミットで、コミットに入る中身（index と
# HEAD の差）を gitleaks で見る。グローバルの pre-commit と pre-merge-commit が、index を変える検査の後に呼ぶ。
# 値は出さず、場所とルール ID だけを出す。漏えいと、検査できないこと（gitleaks の失敗）を分けて出し、どちらも
# 終了コード 1 で止める。迂回は ALLOW_SECRET_COMMIT=<理由>（claude-hooks と同じ規則。ADR-0023）
#
# 検証: modules/home/parts/tests/pre-commit-secrets.sh

[ "${CLAUDECODE:-}" = "1" ] || exit 0

# エスケープの値は理由の文字列。空白を除き、ASCII を 1、それ以外（UTF-8 の先頭バイトで数える）を 2 として幅 6 以上
# なら通す。バックスラッシュは数えない。空白は claude-hooks の char::is_whitespace と同じ Unicode の White_Space（U+0009-000D、0020、0085、00A0、
# 1680、2000-200A、2028、2029、202F、205F、3000）を UTF-8 のバイト列で除く。0 と空は付けていないものとして扱う。
# 短い値は、止めるときに本来の文言に理由を求める行を足す（しきい値は文言に出さない。長さに合わせた無意味な値を
# 誘うため）。展開（$VAR）を含む値は、この hook には展開後の値しか見えないので、claude-hooks の側で止める
reason=$(printf '%s' "${ALLOW_SECRET_COMMIT:-}" \
  | LC_ALL=C sed 's/\xc2[\x85\xa0]//g; s/\xe1\x9a\x80//g; s/\xe2\x80[\x80-\x8a\xa8\xa9\xaf]//g; s/\xe2\x81\x9f//g; s/\xe3\x80\x80//g' \
  | tr -d '[:space:]\134')
escape=absent
if [ -n "$reason" ] && [ "$reason" != "0" ]; then
  ascii=$(printf '%s' "$reason" | LC_ALL=C tr -d '\200-\377' | wc -c)
  lead=$(printf '%s' "$reason" | LC_ALL=C tr -cd '\300-\377' | wc -c)
  if [ $((ascii + 2 * lead)) -ge 6 ]; then
    exit 0
  fi
  escape=short
fi

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
reason_required() {
  [ "$escape" = short ] || return 0
  echo "   ALLOW_SECRET_COMMIT には理由を記載してください（ALLOW_SECRET_COMMIT=<理由> の形で、値はそのまま記録に残ります）"
}
if [ "$rc" -eq 1 ] && [ "${count:-0}" -gt 0 ]; then
  echo "❌ 機密情報らしき内容がコミット対象に含まれています（gitleaks）:"
  printf '%s\n' "$leaks" | jq -r '.[] | "- \(.File):\(.StartLine) (\(.RuleID))"'
  echo "   対処: git restore --staged <file> で外し、必要なら .gitignore に足してください。push 済みの値はローテーションしてください"
  echo "   誤検出なら、その行に gitleaks:allow を書いてください（.gitleaksignore / .gitleaks.toml でも可）"
  echo "   どうしても外すときは、理由を値にして ALLOW_SECRET_COMMIT=<理由> を付けてください（理由はそのまま記録に残ります）。機械で判定できる理由なら hook の不足なので、issue に書いてください"
  reason_required
  exit 1
fi
if [ "$rc" -ne 0 ] || [ -s "$err" ]; then
  echo "❌ gitleaks が失敗しました（終了コード $rc）。機密情報を検査できないので、コミットを止めます:"
  sed 's/^/   /' "$err"
  reason_required
  exit 1
fi
