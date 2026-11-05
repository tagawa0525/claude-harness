# shellcheck shell=bash
# コミットのメッセージ（ファイル）を、Claude Code のセッションのコミットの規約で検査する: Conventional Commits の
# 件名、72 文字の上限（50 文字を超えたら警告）、崩れた日本語の語（ja-terms-message）。git の commit-msg の hook と、
# claude-hooks の git commit の本文を先に検査するルール（check-commit-message）が、同じ判定にするために共有する。
# 検査するかどうか（CLAUDECODE、upstream のある fork、プロジェクトローカルの commit-msg）は呼ぶ側が決める。
#
# 使い方: commit-msg-check <メッセージのファイル>
# 終了コード: 0 = 通す、1 = 規約に反する、2 = 語の検査（textlint）が失敗した（呼ぶ側が判定できない理由として扱う）

# 文字数を UTF-8 の文字単位で数える（LANG=C だとバイト数になり日本語件名が誤って超過する）
export LC_ALL=C.UTF-8

MSG_FILE="$1"

# 最初の非コメント行（sed のみ。grep を挟むとコメント行だけのとき exit 1 で静かに落ちる）
SUBJECT=$(sed -n '/^#/!{p;q}' "$MSG_FILE")

# 日本語の崩れた語を、メッセージのファイルで見る。検査する範囲（件名を除くか、scissors）は
# ja-terms-message（./message.sh）が決める。textlint（Node.js）は起動が重いので、軽い検査の後に呼ぶ
ja_check() {
  local rc=0
  ja-terms-message <"$MSG_FILE" || rc=$?
  if [ "$rc" -eq 1 ]; then
    echo "❌ コミットの本文に崩れた日本語の語があります。上の「誤り => 正しい語」に直してください"
    exit 1
  elif [ "$rc" -ne 0 ]; then
    echo "❌ textlint が失敗しました（終了コード $rc）。上の出力を見てください"
    exit 2
  fi
}

# マージ・fixup/squash・Revert は Conventional Commits の対象外（日本語の語は、ja-terms-message が件名を
# 除いた本文だけを見る）
case "$SUBJECT" in
  Merge* | fixup!* | squash!* | Revert*)
    ja_check
    exit 0
    ;;
esac

TYPES='feat|fix|docs|style|refactor|test|chore|perf|build|ci|revert'
if ! printf '%s\n' "$SUBJECT" | grep -qE "^($TYPES)(\([^)]+\))?!?: [^ ]"; then
  echo "❌ Conventional Commits 形式ではありません: $SUBJECT"
  echo "   形式: <type>(<scope>)?: <subject>    type: $TYPES"
  exit 1
fi

LEN=${#SUBJECT}
if [ "$LEN" -gt 72 ]; then
  echo "❌ 件名が 72 文字を超えています ($LEN 文字): $SUBJECT"
  exit 1
fi
if [ "$LEN" -gt 50 ]; then
  echo "⚠️  件名が 50 文字を超えています ($LEN 文字)。短くできないか検討してください"
fi

ja_check
exit 0
