#!/usr/bin/env bash
# =============================================================================
# git pre-commit hook が、プロジェクトの .pre-commit-config.yaml を prek で走らせ、
# そのうえで既定の検査も走らせることを検証する
# =============================================================================
# ../git.nix の pre-commit は core.hooksPath でグローバル配布される。プロジェクトは
# .pre-commit-config.yaml に hook を足してオプトインする（設計の記録の検査など。ADR-0009）。
# core.hooksPath があると prek install は拒否されるので、グローバルの hook が prek を呼ぶ。
# prek は偽物（呼ばれた引数を記録し、決めた終了コードで終わる）を PATH の先頭に置き、
# 呼ぶかどうか、その結果でコミットを止めるか、既定の検査（ruff）が飛ばないかを見る。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/pre-commit-prek.sh

set -euo pipefail

HOST="${1:-r995}"

if ! command -v ruff >/dev/null 2>&1; then
  echo "✗ ruff が PATH にない。既定の検査が走ったかを確かめられないので、このテストは成立しない"
  exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> pre-commit hook を取り出す（nixosConfigurations.${HOST}）"
nix eval --raw --option warn-dirty false \
  ".#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/pre-commit\".text" \
  > "$WORK/pre-commit"
chmod +x "$WORK/pre-commit"

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main

# 本物の prek（rebuild 後は PATH にある）を見つけないよう、prek のあるディレクトリを PATH から外す
CLEAN_PATH=""
IFS=: read -ra DIRS <<<"$PATH"
for d in "${DIRS[@]}"; do
  [[ -x "$d/prek" ]] || CLEAN_PATH="${CLEAN_PATH:+$CLEAN_PATH:}$d"
done
export PATH="$CLEAN_PATH"

# 偽の prek: 引数を $WORK/prek-args に書き、PREK_EXIT（既定 0）で終わる
FAKE="$WORK/fake-bin"
mkdir -p "$FAKE"
cat >"$FAKE/prek" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >"$WORK/prek-args"
echo "fake prek"
exit "\${PREK_EXIT:-0}"
EOF
chmod +x "$FAKE/prek"

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# 使わない import（F401）はどの版の ruff でも既定で落ちる違反
BAD_PY=$'import os\n'

# repo <name> [config]: ファイルをステージした repo を作る。config を付けると .pre-commit-config.yaml を置く
repo() {
  local dir="$WORK/$1"
  git init -q -b main "$dir"
  if [[ "${2:-}" == config ]]; then
    printf 'repos: []\n' >"$dir/.pre-commit-config.yaml"
  fi
  printf 'ok\n' >"$dir/ok.txt"
  git -C "$dir" add -A
  echo "$dir"
}

# run_hook <repo> [with-prek]: ステージ済みの状態で pre-commit を実行し、終了コードを返す
run_hook() {
  local rc=0 path="$PATH"
  [[ "${2:-}" == with-prek ]] && path="$FAKE:$PATH"
  rm -f "$WORK/prek-args"
  (cd "$1" && PATH="$path" "$WORK/pre-commit") >"$WORK/out" 2>&1 || rc=$?
  return $rc
}

# --- 1. 設定があれば prek を pre-commit の段で走らせ、そのうえで既定の検査も走る ------------
R=$(repo opted config)
printf '%s' "$BAD_PY" >"$R/bad.py"
git -C "$R" add bad.py
if run_hook "$R" with-prek; then
  ng "prek が通っても、既定の検査（ruff）に落ちる Python は止めなければならない"
else
  ok "prek が通っても既定の検査は走り、ruff に落ちる Python を止める"
fi
if [[ "$(cat "$WORK/prek-args" 2>/dev/null)" == "run --hook-stage pre-commit" ]]; then
  ok "prek を run --hook-stage pre-commit で呼ぶ"
else
  ng "prek が run --hook-stage pre-commit で呼ばれていない（引数: $(cat "$WORK/prek-args" 2>/dev/null)）"
fi

# --- 2. prek が失敗すればコミットを止める -------------------------------------------------
R=$(repo failing config)
if PREK_EXIT=1 run_hook "$R" with-prek; then
  ng "prek が失敗したのにコミットが通った"
else
  ok "prek が失敗すればコミットを止める"
fi

# --- 3. 設定が無ければ prek を呼ばない -----------------------------------------------------
R=$(repo plain)
if run_hook "$R" with-prek && [[ ! -e "$WORK/prek-args" ]]; then
  ok "設定が無ければ prek を呼ばずに通る"
else
  ng "設定が無いのに prek が呼ばれたか、フックが失敗した"
fi

# --- 4. 設定があって prek が PATH に無ければ、知らせて既定の検査だけで通す ----------------
R=$(repo missing config)
if run_hook "$R" && grep -q "prek" "$WORK/out"; then
  ok "prek が PATH に無ければ、その旨を出して既定の検査だけで通す"
else
  ng "prek が PATH に無いときに、知らせずに通したか、フックが失敗した"
fi

# --- 5. fork でも、プロジェクトの設定（上流の規約）は走らせる -------------------------------
R=$(repo fork config)
git -C "$R" remote add upstream https://example.invalid/upstream.git
if PREK_EXIT=1 run_hook "$R" with-prek; then
  ng "fork で prek が失敗したのにコミットが通った（上流の設定は上流の規約なので走らせる）"
else
  ok "fork でも prek を走らせ、失敗すればコミットを止める"
fi
if grep -q "飛ばします" "$WORK/out"; then
  ok "fork では、こちらの規約の既定の検査は飛ばす"
else
  ng "fork で既定の検査を飛ばした旨の出力がない"
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
