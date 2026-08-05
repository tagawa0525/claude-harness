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
# 隔離した TMUX_TMPDIR に専用の tmux サーバーを立て、
#   1回目の接続 → デタッチ → 2回目の接続
# を行い、2回目が新規セッションを作らずに未接続セッションを回収したかを判定する。
# =============================================================================
set -euo pipefail

SCRIPT="${1:?usage: local-tmux.sh <path-to-local-tmux>}"

WORK=$(mktemp -d)
export TMUX_TMPDIR="$WORK"
SOCKET="$WORK/tmux-$(id -u)/default"

cleanup() {
  tmux -S "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# サーバー未起動時も空文字を返す（呼び出し側で件数0として扱う）
sessions() {
  tmux -S "$SOCKET" list-sessions \
    -F '#{session_name}|#{session_attached}|#{session_group}' 2>/dev/null | sort || true
}

session_count() {
  sessions | grep -c . || true
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

fail() {
  echo "FAIL: $1"
  echo "--- sessions ---"
  sessions
  exit 1
}

# --- 1回目の接続: main が作られる ---
pid1=$(launch)
wait_for_sessions 1 || fail "1回目の接続でセッションが作られなかった"
sleep 1

[ "$(session_count)" -eq 1 ] || fail "1回目の接続でセッションが1つにならなかった"

# --- デタッチ（ssh が切れた状況を再現）---
# 既に終了している場合があるので失敗は無視する
kill "$pid1" 2>/dev/null || true
tmux -S "$SOCKET" detach-client -s main 2>/dev/null || true
sleep 1

attached=$(tmux -S "$SOCKET" list-sessions -F '#{session_attached}' | paste -sd, -)
[ "$attached" = "0" ] || fail "デタッチ後もクライアントが残っている (attached=$attached)"

# --- 2回目の接続: 未接続の main を回収すべき ---
pid2=$(launch)
sleep 3

count=$(session_count)
echo "--- 2回目の接続後 ---"
sessions

kill "$pid2" 2>/dev/null || true

if [ "$count" -ne 1 ]; then
  fail "未接続セッションを回収せず ${count} 個に増えた（期待値: 1）"
fi

echo "PASS: 未接続セッションを回収し、セッション数は1のまま"
