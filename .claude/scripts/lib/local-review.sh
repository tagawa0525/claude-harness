# shellcheck shell=bash
# local-review.sh — ローカルレビューの記録（record-local-review.sh が PR に付けるコメント）で head が覆われているかを確かめる（source して使う）
#
# local_review_covers <head の SHA>
#   PR_NUMBER の PR の head がローカルレビューの記録で覆われていれば yes、そうでなければ no を出力する（parse_pr_url の
#   後に呼ぶ）。判定は pre-merge-check と同じ実装（claude-hooks local-review-covers）を呼び、記録を bash で読み直さない
#   （読み方が 2 か所にあると食い違い、decide-next.sh が通すと言った head を hook が止める）。head の記録（High なし）が
#   あるか、最後の記録の SHA から rebase だけした head なら yes。rebase だけかは hook と同じく cwd の clone のコミットで比べ
#   （無ければ origin から取る）、PR のリポジトリの clone の外や API や fetch の失敗で比べられなければ no（止める側）。
#   記録を取得できなければ 1 を返す

# shellcheck disable=SC2034  # record-local-review.sh が使う
LOCAL_REVIEW_MARK='<!-- local-review '

local_review_covers() {
  local head="$1" hooks_bin rc=0
  # ~/.claude/bin は claude-code.nix が置く。スクリプトの場所から辿らないのは、リポジトリの checkout から実行しても同じ
  # バイナリを使うため
  hooks_bin="${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}"
  "$hooks_bin" local-review-covers "$GH_REPO" "$PR_NUMBER" "$head" || rc=$?
  case "$rc" in
    0) echo yes ;;
    1) echo no ;;
    127)
      echo "ERROR: ${hooks_bin} がありません。home-manager の rebuild で入れてください（claude-sync はバイナリを入れない）" >&2
      return 1
      ;;
    2)
      # claude-sync はスクリプトだけを入れ替え、~/.claude/bin のバイナリは home-manager の rebuild まで古いまま
      echo "ERROR: ${hooks_bin} が使い方の誤り（終了コード 2）で止まりました。local-review-covers を知らない古いバイナリなら home-manager の rebuild で入れ直してください" >&2
      return 1
      ;;
    *)
      echo "ERROR: ローカルレビューの記録を確かめられませんでした（${hooks_bin} local-review-covers の終了コード ${rc}）" >&2
      return 1
      ;;
  esac
}
