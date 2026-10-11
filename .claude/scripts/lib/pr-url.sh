# shellcheck shell=bash
# pr-url.sh — PR のスクリプトが対象の PR を URL で受けるための解析（source して使う）
#
# parse_pr_url [--allow-fragment] <URL>
#   https://github.com/<owner>/<repo>/pull/<番号>[/<ページ>][?<クエリ>][#<フラグメント>] を解析し、
#   PR_NUMBER を設定して GH_REPO=<owner>/<repo> を export する。gh は GH_REPO を gh pr の対象にも
#   gh api の {owner} / {repo} にも使うので、以降の gh の呼び出しは cwd のリポジトリに依らない（ADR-0032）。
#   /files や通知のクエリの付いた URL も、同じ PR を指すので受ける。
#   # の付いた URL は --allow-fragment のときだけ受け、# 以降（# を除く）を PR_URL_FRAGMENT に入れる。
#   既定で拒むのは、コメントの URL を渡した呼び出しが、別に渡す comment_id と食い違っても気付けないため。
#   受けられなければ理由を stderr に出して 1 を返す（gh は呼ばない）

PR_URL_RE='^https://github\.com/([^/?#]+)/([^/?#]+)/pull/([0-9]+)(/[^?#]*)?(\?[^#]*)?(#(.*))?$'

parse_pr_url() {
  local allow_fragment=0
  if [[ "${1:-}" == "--allow-fragment" ]]; then
    allow_fragment=1
    shift
  fi
  if [[ ! "${1:-}" =~ $PR_URL_RE ]]; then
    echo "ERROR: 対象は PR の URL（https://github.com/<owner>/<repo>/pull/<番号>）で渡してください: '${1:-}'" >&2
    return 1
  fi
  if [[ -n "${BASH_REMATCH[6]}" ]] && ((!allow_fragment)); then
    echo "ERROR: PR の URL に # 以降を付けないでください（コメントは別の引数で渡す）: '${1}'" >&2
    return 1
  fi
  export GH_REPO="${BASH_REMATCH[1]}/${BASH_REMATCH[2]}"
  # shellcheck disable=SC2034  # source した側が使う
  PR_NUMBER="${BASH_REMATCH[3]}"
  # shellcheck disable=SC2034  # source した側が使う
  PR_URL_FRAGMENT="${BASH_REMATCH[7]}"
}
