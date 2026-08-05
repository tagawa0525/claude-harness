#!/usr/bin/env bash
# =============================================================================
# local-tmux の「未接続セッション回収」挙動を検証するテスト
# =============================================================================
# ../tmux.nix の tmuxConnectCmd が対象。
#
# 使い方（リポジトリルートで実行する）:
#   nix build --impure --no-link --print-out-paths --expr '
#     let
#       f = builtins.getFlake (toString ./.);
#       hm = f.nixosConfigurations.t14g4.config.home-manager.users.tagawa;
#       script = builtins.head (
#         builtins.filter (p: (p.name or "") == "local-tmux") hm.home.packages
#       );
#     in [ script hm.xdg.configFile."tmux/tmux.conf".source ]'
#   ./modules/home/parts/tests/local-tmux.sh <1つ目>/bin/local-tmux <2つ目>
#
# tmux.conf を引数で受け取り HOME/XDG_CONFIG_HOME ごと隔離するのは、
# インストール済みの設定を読ませるとホストの rebuild 状況で結果が変わるため。
# destroy-unattached の有無で残る未接続セッションの数が変わるので、必ず
# ブランチの設定で検証する。
#
# シナリオごとに隔離した TMUX_TMPDIR で専用の tmux サーバーを立てて検証する:
#   1. 未接続セッションを回収し、セッション数が増えない
#   2. 未接続セッションが複数ある場合、最後にアタッチしたものを回収する
# =============================================================================
set -euo pipefail

SCRIPT="${1:?usage: local-tmux.sh <path-to-local-tmux> <path-to-tmux.conf>}"
CONF="${2:?usage: local-tmux.sh <path-to-local-tmux> <path-to-tmux.conf>}"

# 前提の欠落は「セッションが増えなかった」と区別がつかず false green になるため、
# テスト本体に入る前に落とす
command -v tmux >/dev/null || { echo "FAIL: tmux が見つからない"; exit 1; }
command -v script >/dev/null || { echo "FAIL: script(util-linux) が見つからない"; exit 1; }
[ -x "$SCRIPT" ] || { echo "FAIL: $SCRIPT が実行可能でない"; exit 1; }
[ -r "$CONF" ] || { echo "FAIL: $CONF が読めない"; exit 1; }

WORK=""
SOCKET=""

teardown() {
  [ -n "$WORK" ] || return 0
  tmux -S "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$WORK"
  WORK=""
}
trap teardown EXIT

setup() {
  teardown
  WORK=$(mktemp -d)
  export TMUX_TMPDIR="$WORK"
  # ホストの ~/.tmux.conf や ~/.config/tmux/tmux.conf を読ませない
  export HOME="$WORK/home"
  export XDG_CONFIG_HOME="$HOME/.config"
  mkdir -p "$XDG_CONFIG_HOME/tmux"
  cp "$CONF" "$XDG_CONFIG_HOME/tmux/tmux.conf"
  SOCKET="$WORK/tmux-$(id -u)/default"
}

# サーバー未起動時も空を返す（呼び出し側で件数0として扱う）
sessions() {
  tmux -S "$SOCKET" list-sessions \
    -F '#{session_name}|#{session_attached}|#{session_group}' 2>/dev/null | sort || true
}

session_count() {
  sessions | grep -c . || true
}

attached_names() {
  sessions | awk -F'|' '$2 == 1 { print $1 }' | paste -sd, -
}

# attach には端末が必要なので疑似端末上で起動する
launch() {
  script -qec "$SCRIPT" /dev/null >/dev/null 2>&1 &
  echo $!
}

wait_for_sessions() {
  local want=$1 i
  for i in $(seq 40); do
    if [ "$(session_count)" -ge "$want" ]; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

# 指定セッションのクライアントを切断する。既に終了している場合は無視
detach() {
  kill "$1" 2>/dev/null || true
  tmux -S "$SOCKET" detach-client -s "$2" 2>/dev/null || true
}

fail() {
  echo "FAIL: $1"
  echo "--- sessions ---"
  sessions
  exit 1
}

# ---------------------------------------------------------------------------
# シナリオ1: 未接続セッションを回収し、セッション数が増えない
# ---------------------------------------------------------------------------
# destroy-unattached の対象外である main は detach 後も残るため、回収しないと
# 接続のたびに main-1 を作り直すことになる
echo "=== シナリオ1: 未接続セッションの回収 ==="
setup

pid1=$(launch)
wait_for_sessions 1 || fail "1回目の接続でセッションが作られなかった"
sleep 1
[ "$(session_count)" -eq 1 ] || fail "1回目の接続でセッションが1つにならなかった"

# ssh が切れた状況を再現
detach "$pid1" main
sleep 1
[ -z "$(attached_names)" ] || fail "デタッチ後もクライアントが残っている ($(attached_names))"
[ "$(session_count)" -eq 1 ] || fail "アンカーの main が残っていない"

# 2回目の接続: 未接続の main を回収すべき
pid2=$(launch)
sleep 3
count=$(session_count)
attached=$(attached_names)
sessions
detach "$pid2" main

# 「起動に失敗してセッションが増えなかっただけ」を PASS と誤判定しないよう、
# 実際にアタッチできていることを先に確認する
[ -n "$attached" ] || fail "2回目の接続がアタッチできていない（起動失敗の可能性）"
[ "$count" -eq 1 ] || fail "未接続セッションを回収せず ${count} 個に増えた（期待値: 1）"
echo "PASS: 未接続セッションを回収し、セッション数は1のまま"

# ---------------------------------------------------------------------------
# シナリオ2: 未接続セッションが複数ある場合、最後にアタッチしたものを回収する
# ---------------------------------------------------------------------------
# destroy-unattached はサーバー起動時に読まれるため、この設定が入る前から
# 動いているサーバーには効かず、未接続セッションが複数残る。その状況で名前順に
# 選ぶと常に main が選ばれ、切断前の current window に戻れない。
#
# 選別キーの session_last_attached は detach では更新されないため、
# アタッチ順と切断順を逆にして「アタッチ時刻で選んでいる」ことを確定させる。
echo "=== シナリオ2: 最後にアタッチしたセッションの回収 ==="
setup

pidA=$(launch)
wait_for_sessions 1 || fail "1クライアント目の接続に失敗した"
sleep 1
# destroy-unattached 導入前から動いているサーバーを再現する
tmux -S "$SOCKET" set-option -g destroy-unattached off

pidB=$(launch)
wait_for_sessions 2 || fail "2クライアント目でグループセッションが作られなかった"
sleep 1
[ "$(session_count)" -eq 2 ] || fail "セッションが2つにならなかった"

# アタッチ順は main → main-1 なので、切断は逆順の main-1 → main にする。
# 切断順で選んでいるなら main、アタッチ順で選んでいるなら main-1 が回収される
detach "$pidB" main-1
sleep 2
detach "$pidA" main
sleep 2
[ -z "$(attached_names)" ] || fail "デタッチ後もクライアントが残っている ($(attached_names))"
[ "$(session_count)" -eq 2 ] || fail "未接続セッションが2つ残っていない"

pidC=$(launch)
sleep 3
count=$(session_count)
attached=$(attached_names)
sessions
detach "$pidC" main-1

[ -n "$attached" ] || fail "3回目の接続がアタッチできていない（起動失敗の可能性）"
[ "$count" -eq 2 ] || fail "セッションが ${count} 個に増えた（期待値: 2）"
[ "$attached" = "main-1" ] || fail "最後にアタッチした main-1 ではなく ${attached} を回収した"
echo "PASS: 最後にアタッチした main-1 を回収した（切断は main-1 が先）"

echo "すべてのシナリオが PASS"
