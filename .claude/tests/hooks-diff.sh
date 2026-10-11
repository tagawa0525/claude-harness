#!/usr/bin/env bash
# claude-hooks の 2 つの版の判定を、同じコマンドの集まりで比べる
#
# 実行: bash .claude/tests/hooks-diff.sh [BASE]
#   BASE（既定 origin/main）の版と、作業ツリーの版（git が追跡するファイル。新しいファイルは git add -N で
#   見せる）を nix build して比べる。OLD_BIN / NEW_BIN を渡すと build せずにそのバイナリを使う。
#   HOOKS_DIFF_COMMANDS（改行区切り）を渡すと、組み合わせの集まりの代わりにそのコマンドだけを比べる
#   HOOKS_DIFF_LIST=1 を渡すと、build もバイナリの呼び出しもせずに、比べる集まり
#   （<background の有無><TAB><コマンド>）だけを出す
#
# 出力（タブ区切りの行と、KEY: value）:
#   LOOSENED<TAB><ルール><TAB><コマンド>   古い版が止め、新しい版が通す（意図した変更か、後退か）。
#                                          新しい版で無くなったルールは、コマンドの代わりに「(新しい版に無い)」
#   TIGHTENED<TAB><ルール><TAB><コマンド>  古い版が通し、新しい版が止める
#   CHANGED<TAB><ルール><TAB><古> -> <新><TAB><コマンド>  deny 以外の判定の変化（context は additionalContext）
#   NEW_RULE: <ルール>                     古い版に無いルール（比べない）
#   UNCOVERED: <ルール>                    古い版が一度も通す以外を返さなかったルール（緩んでも出ない）
#   CHECKED: / LOOSENED: / TIGHTENED: / CHANGED:  比べたコマンドの数と、それぞれの行の数
#
# 判定がリポジトリの状態で決まるルールのために、GitHub のリモートがある main のリポジトリで走らせ、
# リモートの無い別のリポジトリへ cd / -C で移る形も比べる。main では block-main-commit が全てのコミットを
# 止めるので、ルールをまとめて判定すると他のルールの緩みが隠れる。ルールごとに比べる。

set -euo pipefail

BASE="${1:-origin/main}"

# lib.sh が HOME を差し替える前に build する（nix の設定とキャッシュを使うため）
build() {
  nix build --no-link --print-out-paths --option warn-dirty false "$1#claude-hooks"
}
if [[ -z "${HOOKS_DIFF_LIST:-}" ]] && [[ -z "${OLD_BIN:-}" || -z "${NEW_BIN:-}" ]]; then
  # 呼んだ場所ではなく、このスクリプトのある作業ツリーを比べる
  ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
  # path: は .gitignore を見ずに target/ なども store に写すので、git が追跡するファイルだけを使う
  OLD_BIN="${OLD_BIN:-$(build "git+file://$ROOT?rev=$(git -C "$ROOT" rev-parse "$BASE")")/bin/claude-hooks}"
  NEW_BIN="${NEW_BIN:-$(build "git+file://$ROOT")/bin/claude-hooks}"
fi

source "$(dirname "$0")/lib.sh"

# 判定の場所: main（GitHub のリモートあり、feat/x あり）と、リモートの無いリポジトリ。gh は偽物で、
# 呼ばれたら失敗する（ネットワークに出ない）
MAIN="$TEST_ROOT/main" LOCAL="$TEST_ROOT/local"
make_repo "$MAIN"
make_remote "$MAIN" github >/dev/null
make_repo "$LOCAL"
git -C "$MAIN" branch feat/x
mkdir -p "$MAIN/src"
echo x >"$MAIN/a.txt"
make_fake_gh ''
# gitleaks は常に漏えいを返す（本物は漏えいの無い一時のリポジトリで必ず通し、block-secret-commit を比べられない）
make_fake_tool gitleaks "  *) echo '[{\"File\":\"a.txt\",\"StartLine\":1,\"RuleID\":\"x\"}]'; exit 1 ;;"
cd "$MAIN"

COMMANDS=(
  "git commit -m x" "git commit -am x" "git commit --all -m x" "git commit -n -m x" "git commit -anm x"
  "git commit --no-verify -m x" "git commit --no-v -m x" "git commit -m -n" "git commit -m --no-verify"
  "git commit -mx -n" "git commit -m x -- a.txt" "git -c core.hooksPath=/dev/null commit -m x"
  "git --config-env=core.hooksPath=X commit -m x" "git -C $LOCAL commit -m x" "git --attr-source HEAD commit -n -m x"
  "git merge feat/x" "git merge --no-ff feat/x" "git merge --no-verify feat/x" "git merge -m --no-verify feat/x"
  "git merge --no-ff --squash feat/x" "git pull" "git pull --no-verify" "git pull -j --no-verify"
  "git push origin main" "git push --force origin x" "git push -f origin x" "git push --all" "git push --mirror"
  "git push -o --force origin x" "git push origin +x" "git push --force-with-lease"
  "git add -A" "git add ." "git add --all" "git add :/" "git add a.txt" "git add -A src" "git add -u" "git add -- ."
  "git add -A --pathspec-from-file=p" "git add --pathspec-from-file=p"
  "git config core.hooksPath /x" "git config --get core.hooksPath"
  "sbatch x.sh" "bash slurm-run.sh -t 1 n x" "slurm-run.sh -t 1 n x"
)
WRAPPERS=(
  "" "env " "env -- FOO=1 " "env -- CLAUDECODE=0 " "unset CLAUDECODE && " "env -i " "env -i CLAUDECODE=1 " "env -u CLAUDECODE " "env CLAUDECODE=0 "
  "CLAUDECODE=0 " "GIT_CONFIG_COUNT=1 " "env -S 'FOO=1' " "/usr/bin/env " "env -C $LOCAL " "command " "command -v "
  "builtin " "exec " "exec -c " "nohup " "time " "/usr/bin/time " "nice -n 5 " "timeout 10 " "setsid "
  "stdbuf -o0 " "ionice -c3 "
)
DIRS=("cd $LOCAL; " "cd $LOCAL && " "env cd $LOCAL; " "builtin cd $LOCAL && " "time cd $LOCAL; " "(cd $LOCAL); "
  "echo \$(cd $LOCAL); " "cd $LOCAL; cd $MAIN; ")

# decide <bin> <rule>: $INPUT への判定を DECISION に入れる（deny / ask / context / allow）。
# バイナリが失敗したら、通したものと数えずに止める。claude-hooks はルールの panic を拾って終了コード 0 で
# 終わる（panic の文は標準エラーに出る）ので、標準エラーに何か出たら失敗とみなす。判定できなかった理由
# （systemMessage）を返したときと、出力が 1 つの JSON の object でないときも失敗とみなす
decide() {
  local bin="$1" rule="$2" out
  if ! out=$(printf '%s' "$INPUT" | "$bin" pre-tool-use --rule "$rule" 2>"$TEST_ROOT/stderr") ||
    [[ -s "$TEST_ROOT/stderr" ]] ||
    { [[ -n "$out" ]] && ! jq -se 'length == 1 and (.[0] | type == "object" and (has("systemMessage") | not))' <<<"$out" >/dev/null 2>&1; }; then
    echo "ERROR: $bin が失敗しました（--rule $rule）: $INPUT" >&2
    cat "$TEST_ROOT/stderr" >&2
    [[ -z "$out" ]] || printf '%s\n' "$out" >&2
    exit 1
  fi
  DECISION=$(decision "$out" context)
}

checked=0
# 古い版が通す以外を返したルール。一度も返さないルールは、緩んでも LOOSENED に出ない
declare -A COVERED=()
compare() {
  local cmd="$1" r old
  INPUT=$(hook_input "$cmd" "$2")
  checked=$((checked + 1))
  for r in "${RULES[@]}"; do
    decide "$OLD_BIN" "$r"
    old="$DECISION"
    [[ "$old" != allow ]] && COVERED[$r]=1
    decide "$NEW_BIN" "$r"
    [[ "$old" == "$DECISION" ]] && continue
    if [[ "$old" == deny ]]; then
      printf 'LOOSENED\t%s\t%s\n' "$r" "$cmd"
      loosened=$((loosened + 1))
    elif [[ "$DECISION" == deny ]]; then
      printf 'TIGHTENED\t%s\t%s\n' "$r" "$cmd"
      tightened=$((tightened + 1))
    else
      printf 'CHANGED\t%s\t%s -> %s\t%s\n' "$r" "$old" "$DECISION" "$cmd"
      changed=$((changed + 1))
    fi
  done
}

# 比べる集まり: <background の有無><TAB><コマンド>
CASES=()
if [[ -n "${HOOKS_DIFF_COMMANDS:-}" ]]; then
  while IFS= read -r c; do
    [[ -n "$c" ]] && CASES+=("false"$'\t'"$c")
  done <<<"$HOOKS_DIFF_COMMANDS"
else
  for c in "${COMMANDS[@]}"; do
    for w in "${WRAPPERS[@]}"; do
      CASES+=("false"$'\t'"$w$c")
    done
    for d in "${DIRS[@]}"; do
      CASES+=("false"$'\t'"$d$c")
    done
    CASES+=("true"$'\t'"$c")
  done
fi
if [[ -n "${HOOKS_DIFF_LIST:-}" ]]; then
  printf '%s\n' "${CASES[@]}"
  exit 0
fi

# ルールの一覧（引数なしで呼ぶと使い方に出る）
rules_of() {
  { "$1" 2>&1 || true; } | sed -n 's/^rules: //p' | tr -d ' ' | tr ',' '\n'
}
mapfile -t OLD_RULES < <(rules_of "$OLD_BIN")
mapfile -t NEW_RULES < <(rules_of "$NEW_BIN")
if [[ ${#OLD_RULES[@]} -eq 0 || ${#NEW_RULES[@]} -eq 0 ]]; then
  echo "ERROR: ルールの一覧を読めません: $OLD_BIN / $NEW_BIN" >&2
  exit 1
fi
has() {
  local x="$1"
  shift
  printf '%s\n' "$@" | grep -qx -- "$x"
}
RULES=()
loosened=0 tightened=0 changed=0
for r in "${OLD_RULES[@]}"; do
  if has "$r" "${NEW_RULES[@]}"; then
    RULES+=("$r")
  else
    printf 'LOOSENED\t%s\t(新しい版に無い)\n' "$r"
    loosened=$((loosened + 1))
  fi
done
for r in "${NEW_RULES[@]}"; do
  has "$r" "${OLD_RULES[@]}" || echo "NEW_RULE: $r"
done

for case in "${CASES[@]}"; do
  compare "${case#*$'\t'}" "${case%%$'\t'*}"
done

for r in "${RULES[@]}"; do
  [[ -n "${COVERED[$r]:-}" ]] || echo "UNCOVERED: $r"
done
echo "CHECKED: $checked"
echo "LOOSENED: $loosened"
echo "TIGHTENED: $tightened"
echo "CHANGED: $changed"
