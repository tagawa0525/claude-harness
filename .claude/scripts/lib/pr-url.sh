# shellcheck shell=bash
# pr-url.sh — PR のスクリプトが対象の PR を URL で受けるための解析（source して使う）
#
# parse_pr_url <URL>
#   https://github.com/<owner>/<repo>/pull/<番号>[/][#...] を解析し、PR_NUMBER を設定して
#   GH_REPO=<owner>/<repo> を export する。gh は GH_REPO を gh pr の対象にも gh api の
#   {owner} / {repo} にも使うので、以降の gh の呼び出しは cwd のリポジトリに依らない（ADR-0032）。
#   PR_URL_FRAGMENT には # 以降（# を除く。無ければ空）を入れる。
#   URL でなければ理由を stderr に出して 1 を返す（gh は呼ばない）

PR_URL_RE='^https://github\.com/([^/#]+)/([^/#]+)/pull/([0-9]+)/?(#(.*))?$'

parse_pr_url() {
  if [[ ! "${1:-}" =~ $PR_URL_RE ]]; then
    echo "ERROR: 対象は PR の URL（https://github.com/<owner>/<repo>/pull/<番号>）で渡してください: '${1:-}'" >&2
    return 1
  fi
  export GH_REPO="${BASH_REMATCH[1]}/${BASH_REMATCH[2]}"
  PR_NUMBER="${BASH_REMATCH[3]}"
  PR_URL_FRAGMENT="${BASH_REMATCH[5]}"
}
