#!/usr/bin/env bash
# =============================================================================
# local-zellij の「スロット式タブ運用」挙動を検証するテスト
# =============================================================================
# ../zellij.nix の設定と zellij-slots プラグインが対象。
#
# tmux の window 番号運用（renumber-windows off + 優先順位 3 4 2 8 7 9 5 6 1 0）
# を Zellij で再現する。Zellij のタブは位置ベースで固定番号を持てないため、
# タブ「名」をスロット番号として使い、プラグインが空きスロットへの作成と
# 名前ジャンプを担う。
#
# 使い方（リポジトリルートで実行する。zellij が PATH に必要）:
#   nix build --impure --no-link --print-out-paths --expr '
#     let
#       f = builtins.getFlake (toString ./.);
#       hm = f.nixosConfigurations.t14g4.config.home-manager.users.tagawa;
#       script = builtins.head (
#         builtins.filter (p: (p.name or "") == "local-zellij") hm.home.packages
#       );
#     in [
#       script
#       hm.xdg.configFile."zellij/config.kdl".source
#       hm.xdg.configFile."zellij/layouts/slots.kdl".source
#     ]'
#   nix shell nixpkgs#zellij -c \
#     ./modules/home/parts/tests/local-zellij.sh \
#     <1つ目>/bin/local-zellij <2つ目> <3つ目>
#
# config.kdl / layout を引数で受け取り HOME/XDG ごと隔離するのは、
# インストール済みの設定を読ませるとホストの rebuild 状況で結果が変わるため。
#
# シナリオ:
#   1. 初回起動でセッション main が作られ、初期タブ名が「3」になる
#   2. 「new」パイプで優先順位どおり 4, 2, 8 のスロットにタブが増える
#   3. タブを閉じても他のタブ名が変わらず、次の new は空いた番号を再利用する
#   4. 「goto:N」パイプで名前指定のタブへフォーカスが移る
#   5. デタッチしてもセッションが残り、local-zellij で同セッションに再接続する
# =============================================================================
set -euo pipefail

SCRIPT="${1:?usage: local-zellij.sh <path-to-local-zellij> <config.kdl> <slots.kdl>}"
CONF="${2:?usage: local-zellij.sh <path-to-local-zellij> <config.kdl> <slots.kdl>}"
LAYOUT="${3:?usage: local-zellij.sh <path-to-local-zellij> <config.kdl> <slots.kdl>}"

# 前提の欠落は「タブが増えなかった」と区別がつかず false green になるため、
# テスト本体に入る前に落とす
command -v zellij >/dev/null || { echo "FAIL: zellij が見つからない"; exit 1; }
command -v script >/dev/null || { echo "FAIL: script(util-linux) が見つからない"; exit 1; }
[ -x "$SCRIPT" ] || { echo "FAIL: $SCRIPT が実行可能でない"; exit 1; }
[ -r "$CONF" ] || { echo "FAIL: $CONF が読めない"; exit 1; }
[ -r "$LAYOUT" ] || { echo "FAIL: $LAYOUT が読めない"; exit 1; }

# プラグインの権限プロンプトは対話でしか承認できないため、設定とレイアウトから
# wasm のパス（zellij-slotsとzjstatus）を取り出して権限キャッシュを事前に与える
WASMS=$(grep -oh 'file:[^"]*\.wasm' "$CONF" "$LAYOUT" | sed 's/^file://' | sort -u)
[ -n "$WASMS" ] || { echo "FAIL: レイアウトからプラグインのパスを特定できない"; exit 1; }
while IFS= read -r wasm; do
  [ -r "$wasm" ] || { echo "FAIL: プラグイン $wasm が読めない"; exit 1; }
done <<< "$WASMS"

WORK=""

teardown() {
  if [ -n "$WORK" ]; then
    zellij kill-all-sessions --yes >/dev/null 2>&1 || true
    rm -rf "$WORK"
    WORK=""
  fi
}
trap teardown EXIT

setup() {
  teardown
  WORK=$(mktemp -d)
  # zellij内から実行された場合に既存サーバーへ繋がないよう環境を隔離する
  unset ZELLIJ ZELLIJ_SESSION_NAME ZELLIJ_PANE_ID
  export HOME="$WORK/home"
  export XDG_CONFIG_HOME="$HOME/.config"
  export XDG_CACHE_HOME="$HOME/.cache"
  export XDG_DATA_HOME="$HOME/.local/share"
  export ZELLIJ_SOCKET_DIR="$WORK/socket"
  mkdir -p "$XDG_CONFIG_HOME/zellij/layouts" "$XDG_CACHE_HOME/zellij" \
    "$XDG_DATA_HOME" "$ZELLIJ_SOCKET_DIR"
  cp "$CONF" "$XDG_CONFIG_HOME/zellij/config.kdl"
  cp "$LAYOUT" "$XDG_CONFIG_HOME/zellij/layouts/slots.kdl"
  # 権限キャッシュを事前投入（ノード名はプラグインの絶対パス）。
  # 要求される権限がキャッシュを上回るとプロンプトが出て詰まるため、
  # 実際の要求より広めに与えておく
  : > "$XDG_CACHE_HOME/zellij/permissions.kdl"
  while IFS= read -r wasm; do
    cat >> "$XDG_CACHE_HOME/zellij/permissions.kdl" <<EOF
"$wasm" {
    ReadApplicationState
    ChangeApplicationState
    RunCommands
    ReadCliPipes
    MessageAndLaunchOtherPlugins
}
EOF
  done <<< "$WASMS"
}

# attach には端末が必要なので疑似端末上で起動する。
# script -c は文字列をシェルで解釈するため、パスは中でもクォートする
launch() {
  script -qec "'$SCRIPT'" /dev/null </dev/null >/dev/null 2>&1 &
  echo $!
}

Z() {
  zellij --session main action "$@"
}

# タブ名を作成順で1行ずつ返す（セッション未起動なら空）
tab_names() {
  Z query-tab-names 2>/dev/null || true
}

tab_names_csv() {
  tab_names | paste -sd, -
}

# フォーカス中のタブ名。dump-layout の tab 行から focus=true を探す
focused_tab() {
  Z dump-layout 2>/dev/null \
    | grep -E '^\s*tab ' | grep 'focus=true' \
    | grep -o 'name="[^"]*"' | head -n1 | cut -d'"' -f2 || true
}

# パイプでプラグインへメッセージを送る
pipe_slots() {
  zellij --session main pipe --name slots -- "$1"
}

wait_for_tabs() {
  local want=$1 i
  for i in $(seq 40); do
    if [ "$(tab_names | grep -c .)" -ge "$want" ]; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

sessions() {
  zellij list-sessions --short 2>/dev/null || true
}

fail() {
  echo "FAIL: $1"
  echo "--- sessions ---"
  sessions
  echo "--- tabs ---"
  tab_names
  exit 1
}

# ---------------------------------------------------------------------------
# シナリオ1: 初回起動でセッション main が作られ、初期タブ名が「3」になる
# ---------------------------------------------------------------------------
echo "=== シナリオ1: 初回起動と初期スロット ==="
setup

pid1=$(launch)
wait_for_tabs 1 || fail "初回起動でセッションが作られなかった"
sleep 1
[ "$(sessions)" = "main" ] || fail "セッション名が main でない ($(sessions))"
[ "$(tab_names_csv)" = "3" ] || fail "初期タブ名が 3 でない ($(tab_names_csv))"
echo "PASS: セッション main、初期タブ 3"

# ---------------------------------------------------------------------------
# シナリオ2: 「new」パイプで優先順位どおり 4, 2, 8 のスロットにタブが増える
# ---------------------------------------------------------------------------
echo "=== シナリオ2: 優先順位による新規タブ作成 ==="
pipe_slots new
wait_for_tabs 2 || fail "new でタブが増えなかった"
pipe_slots new
wait_for_tabs 3 || fail "2回目の new でタブが増えなかった"
pipe_slots new
wait_for_tabs 4 || fail "3回目の new でタブが増えなかった"
sleep 1
[ "$(tab_names_csv)" = "3,4,2,8" ] \
  || fail "優先順位どおりに増えていない ($(tab_names_csv) 期待値: 3,4,2,8)"
echo "PASS: 3 → 4 → 2 → 8 の順にタブが増えた"

# ---------------------------------------------------------------------------
# シナリオ3: タブを閉じても他のタブ名が変わらず、次の new は空き番号を再利用
# ---------------------------------------------------------------------------
# tmux の renumber-windows off 相当。位置は詰まってもスロット名は動かない
echo "=== シナリオ3: タブ削除後の番号安定性と再利用 ==="
pipe_slots goto:4
sleep 1
[ "$(focused_tab)" = "4" ] || fail "goto:4 でフォーカスが移らなかった ($(focused_tab))"
Z close-tab
sleep 1
[ "$(tab_names_csv)" = "3,2,8" ] \
  || fail "タブ4を閉じた後に名前が変わった ($(tab_names_csv) 期待値: 3,2,8)"
pipe_slots new
wait_for_tabs 4 || fail "削除後の new でタブが増えなかった"
sleep 1
[ "$(tab_names_csv)" = "3,2,8,4" ] \
  || fail "空きスロット 4 を再利用しなかった ($(tab_names_csv) 期待値: 3,2,8,4)"
echo "PASS: タブ名は安定し、空いた 4 を再利用した"

# ---------------------------------------------------------------------------
# シナリオ4: 「goto:N」パイプで名前指定のタブへフォーカスが移る
# ---------------------------------------------------------------------------
echo "=== シナリオ4: 名前ジャンプ ==="
pipe_slots goto:2
sleep 1
[ "$(focused_tab)" = "2" ] || fail "goto:2 でフォーカスが移らなかった ($(focused_tab))"
pipe_slots goto:3
sleep 1
[ "$(focused_tab)" = "3" ] || fail "goto:3 でフォーカスが移らなかった ($(focused_tab))"
echo "PASS: goto でタブ間を移動できた"

# ---------------------------------------------------------------------------
# シナリオ5: デタッチ後もセッションが残り、local-zellij で再接続する
# ---------------------------------------------------------------------------
echo "=== シナリオ5: デタッチと再接続 ==="
kill "$pid1" 2>/dev/null || true
sleep 2
[ "$(sessions)" = "main" ] || fail "デタッチ後にセッションが残っていない ($(sessions))"

pid2=$(launch)
sleep 3
[ "$(sessions)" = "main" ] || fail "再接続でセッションが増減した ($(sessions))"
[ "$(tab_names_csv)" = "3,2,8,4" ] \
  || fail "再接続後にタブ構成が変わった ($(tab_names_csv))"
kill "$pid2" 2>/dev/null || true
echo "PASS: デタッチ後もセッションが残り、同セッションに再接続した"

echo "すべてのシナリオが PASS"
