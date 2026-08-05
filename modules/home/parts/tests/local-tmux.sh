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
#       ps = f.nixosConfigurations.t14g4.config.home-manager.users.tagawa.home.packages;
#     in builtins.head (builtins.filter (p: (p.name or "") == "local-tmux") ps)'
#   ./modules/home/parts/tests/local-tmux.sh <上記のstore path>/bin/local-tmux
#
# シナリオごとに隔離した TMUX_TMPDIR で専用の tmux サーバーを立てて検証する:
#   1. 未接続セッションを回収し、セッション数が増えない
#   2. 未接続セッションが複数ある場合、最後に切断したものを回収する
# =============================================================================
set -euo pipefail

SCRIPT="${1:?usage: local-tmux.sh <path-to-local-tmux>}"

# 前提の欠落は「セッションが増えなかった」と区別がつかず false green になるため、
# テスト本体に入る前に落とす
command -v tmux >/dev/null || { echo "FAIL: tmux が見つからない"; exit 1; }
command -v script >/dev/null || { echo "FAIL: script(util-linux) が見つからない"; exit 1; }
[ -x "$SCRIPT" ] || { echo "FAIL: $SCRIPT が実行可能でない"; exit 1; }

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
# シナリオ2: 未接続セッションが複数ある場合、最後に切断したものを回収する
# ---------------------------------------------------------------------------
# 切断前の current window に戻れることが回収の利点なので、名前順ではなく
# 最終アタッチ時刻で選ぶ必要がある
echo "=== シナリオ2: 最後に切断したセッションの回収 ==="
setup

pidA=$(launch)
wait_for_sessions 1 || fail "1クライアント目の接続に失敗した"
sleep 1
pidB=$(launch)
wait_for_sessions 2 || fail "2クライアント目でグループセッションが作られなかった"
sleep 1
[ "$(session_count)" -eq 2 ] || fail "セッションが2つにならなかった"

# main → main-1 の順に切断する（main-1 が「最後に切断」になる）
detach "$pidA" main
sleep 2
detach "$pidB" main-1
sleep 2
[ -z "$(attached_names)" ] || fail "デタッチ後もクライアントが残っている ($(attached_names))"

pidC=$(launch)
sleep 3
count=$(session_count)
attached=$(attached_names)
sessions
detach "$pidC" main-1

[ -n "$attached" ] || fail "3回目の接続がアタッチできていない（起動失敗の可能性）"
[ "$count" -eq 2 ] || fail "セッションが ${count} 個に増えた（期待値: 2）"
[ "$attached" = "main-1" ] || fail "最後に切断した main-1 ではなく ${attached} を回収した"
echo "PASS: 最後に切断した main-1 を回収した"

echo "すべてのシナリオが PASS"
