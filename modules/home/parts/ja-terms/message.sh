# コミットのメッセージの崩れた日本語の語を検査する（ADR-0014）。git の commit-msg の hook と、claude-hooks の
# git merge の本文を先に検査するルール（ADR-0050）が、同じ判定にするために共有する。メッセージは標準入力で受け、
# JA_TERMS_CONFIG（textlint の設定）は textlint.nix の writeShellApplication が与える。
#
# 件名が Merge / fixup! / squash! / Revert なら件名を除く（元のコミットからコピーしたもの、Merge は git が作るもの。
# fixup / squash の本文は畳まれて履歴に残る）。件名は # で始まらない最初の行で、除くのは空でも # でもない最初の行。
# commit -v の scissors の行から後（差分）は除く。# で始まる行は、-m や -F（cleanup=whitespace）では消えずに
# コミットに残るので、除かない。
# 検査する本文が空白だけのときは textlint を呼ばない。標準入力が空だと、textlint は検査せずに使い方の全文を出す
# （件名を除いた fixup!、squash!、Merge の本文が空の場合）
#
# 終了コード: textlint のもの（0 = 崩れた語が無い、1 = 崩れた語がある、ほか = textlint が失敗した）

msg=$(cat)
subject=$(sed -n '/^#/!{p;q}' <<<"$msg")
case "$subject" in
  Merge* | fixup!* | squash!* | Revert*)
    msg=$(sed '0,/^[^#]/{//d}' <<<"$msg")
    ;;
esac
body=$(sed '/^# -\{24\} >8 -\{24\}$/,$d' <<<"$msg")
[[ "$body" =~ [^[:space:]] ]] || exit 0
printf '%s\n' "$body" |
  textlint --config "$JA_TERMS_CONFIG" --no-color --stdin --stdin-filename COMMIT_EDITMSG.txt
