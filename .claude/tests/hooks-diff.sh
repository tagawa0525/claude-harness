#!/usr/bin/env bash
# claude-hooks の 2 つの版の判定を、同じコマンドの集まりで比べる
#
# 実行: bash .claude/tests/hooks-diff.sh [BASE]
#   BASE（既定 origin/main）の版と、作業ツリーの版を nix build して比べる。OLD_BIN / NEW_BIN を渡すと
#   build せずにそのバイナリを使う
#
# 出力（KEY: value と、タブ区切りの行）:
#   LOOSENED<TAB><ルール><TAB><コマンド>   古い版が止め、新しい版が通す（意図した変更か、後退か）
#   TIGHTENED<TAB><ルール><TAB><コマンド>  古い版が通し、新しい版が止める
#   CHECKED: / LOOSENED: / TIGHTENED:     比べたコマンドの数と、それぞれの行の数
#
# 判定がリポジトリの状態で決まるルールのために、GitHub のリモートがある main のリポジトリで走らせ、
# リモートの無い別のリポジトリへ cd / -C で移る形も比べる。gh は偽物に差し替え、ネットワークに出ない。

set -euo pipefail

BASE="${1:-origin/main}"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

build() {
  local flake="$1"
  nix build --no-link --print-out-paths "$flake#claude-hooks" | tail -1
}

if [[ -z "${OLD_BIN:-}" || -z "${NEW_BIN:-}" ]]; then
  ROOT=$(git rev-parse --show-toplevel)
  OLD_BIN="${OLD_BIN:-$(build "git+file://$ROOT?rev=$(git -C "$ROOT" rev-parse "$BASE")")/bin/claude-hooks}"
  NEW_BIN="${NEW_BIN:-$(build "path:$ROOT")/bin/claude-hooks}"
fi

# ルールの一覧（引数なしで呼ぶと使い方に出る）。新しい版のルールを、古い版にもあるものだけ比べる
rules_of() {
  { "$1" 2>&1 || true; } | sed -n 's/^rules: //p' | tr -d ' ' | tr ',' '\n'
}
mapfile -t OLD_RULES < <(rules_of "$OLD_BIN")
mapfile -t NEW_RULES < <(rules_of "$NEW_BIN")
if [[ ${#OLD_RULES[@]} -eq 0 || ${#NEW_RULES[@]} -eq 0 ]]; then
  echo "ERROR: ルールの一覧を読めません: $OLD_BIN / $NEW_BIN" >&2
  exit 1
fi
RULES=()
for r in "${NEW_RULES[@]}"; do
  if printf '%s\n' "${OLD_RULES[@]}" | grep -qx -- "$r"; then
    RULES+=("$r")
  else
    echo "NEW_RULE: $r"
  fi
done

# 判定の場所: main（GitHub のリモートあり、feat/x あり）と、リモートの無いリポジトリ
export HOME="$WORK/home" GIT_CONFIG_NOSYSTEM=1 GH_CONFIG_DIR="$WORK/gh"
mkdir -p "$HOME" "$WORK/bin"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
git config --global user.name t
git config --global user.email t@example.com
git config --global init.defaultBranch main
# 準備のコミットで、手元の git の hook を走らせない
git config --global core.hooksPath /dev/null
printf '#!/bin/sh\necho "gh: offline" >&2\nexit 1\n' >"$WORK/bin/gh"
chmod +x "$WORK/bin/gh"
export PATH="$WORK/bin:$PATH" CLAUDECODE=1
MAIN="$WORK/main" LOCAL="$WORK/local"
for repo in "$MAIN" "$LOCAL"; do
  git init -q "$repo"
  git -C "$repo" commit -q --allow-empty -m "chore: init"
done
git -C "$MAIN" remote add origin https://github.com/example/x.git
git -C "$MAIN" branch feat/x
mkdir -p "$MAIN/src"
echo x >"$MAIN/a.txt"
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
  "" "env " "env -- FOO=1 " "env -i " "env -i CLAUDECODE=1 " "env -u CLAUDECODE " "env CLAUDECODE=0 "
  "CLAUDECODE=0 " "GIT_CONFIG_COUNT=1 " "env -S 'FOO=1' " "/usr/bin/env " "env -C $LOCAL " "command " "command -v "
  "builtin " "exec " "exec -c " "nohup " "time " "/usr/bin/time " "nice -n 5 " "timeout 10 " "setsid "
  "stdbuf -o0 " "ionice -c3 "
)
DIRS=("cd $LOCAL; " "cd $LOCAL && " "env cd $LOCAL; " "builtin cd $LOCAL && " "time cd $LOCAL; " "(cd $LOCAL); "
  "echo \$(cd $LOCAL); " "cd $LOCAL; cd $MAIN; ")

# 判定（deny / ask / allow など）。$1 バイナリ、$2 コマンド、$3 background、残りは --rule の名前
decide() {
  local bin="$1" cmd="$2" bg="$3" out
  shift 3
  local args=()
  for r in "$@"; do args+=(--rule "$r"); done
  out=$(jq -n --arg cmd "$cmd" --argjson bg "$bg" \
    '{hook_event_name: "PreToolUse", tool_name: "Bash", cwd: env.PWD,
      tool_input: {command: $cmd, run_in_background: $bg}}' |
    "$bin" pre-tool-use "${args[@]}")
  if [[ -z "$out" ]]; then
    echo allow
  else
    jq -r '.hookSpecificOutput.permissionDecision // "context"' <<<"$out"
  fi
}

checked=0 loosened=0 tightened=0
compare() {
  local cmd="$1" bg="$2"
  checked=$((checked + 1))
  [[ "$(decide "$OLD_BIN" "$cmd" "$bg" "${RULES[@]}")" == "$(decide "$NEW_BIN" "$cmd" "$bg" "${RULES[@]}")" ]] && return
  # 全体の判定が変わったコマンドだけ、ルールごとに調べる
  local r old new
  for r in "${RULES[@]}"; do
    old=$(decide "$OLD_BIN" "$cmd" "$bg" "$r")
    new=$(decide "$NEW_BIN" "$cmd" "$bg" "$r")
    if [[ "$old" == deny && "$new" != deny ]]; then
      printf 'LOOSENED\t%s\t%s\n' "$r" "$cmd"
      loosened=$((loosened + 1))
    elif [[ "$old" != deny && "$new" == deny ]]; then
      printf 'TIGHTENED\t%s\t%s\n' "$r" "$cmd"
      tightened=$((tightened + 1))
    fi
  done
}

for c in "${COMMANDS[@]}"; do
  for w in "${WRAPPERS[@]}"; do
    compare "$w$c" false
  done
  for d in "${DIRS[@]}"; do
    compare "$d$c" false
  done
  compare "$c" true
done

echo "CHECKED: $checked"
echo "LOOSENED: $loosened"
echo "TIGHTENED: $tightened"
