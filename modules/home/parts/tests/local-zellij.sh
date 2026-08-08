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
#       cfg = f.nixosConfigurations.t14g4;
#       hm = cfg.config.home-manager.users.tagawa;
#       script = builtins.head (
#         builtins.filter (p: (p.name or "") == "local-zellij") hm.home.packages
#       );
#     in [
#       script
#       hm.xdg.configFile."zellij/config.kdl".source
#       hm.xdg.configFile."zellij/layouts/slots.kdl".source
#       (cfg.pkgs.writeScript "zellij-perm-seed"
#         hm.home.activation.zellijPluginPermissions.data)
#     ]'
#   nix shell nixpkgs#zellij -c \
#     ./modules/home/parts/tests/local-zellij.sh \
#     <1つ目>/bin/local-zellij <2つ目> <3つ目> <4つ目>
#
# config.kdl / layout を引数で受け取り HOME/XDG ごと隔離するのは、
# インストール済みの設定を読ませるとホストの rebuild 状況で結果が変わるため。
#
# シナリオ:
#   0. 権限シードが不完全な既存エントリを補正し、他のエントリを保持する
#   1. 初回起動でセッション main が作られ、初期タブ名が「3」になる
#   2. prefix+c で優先順位どおり 4, 2, 8 のスロットにタブが増える
#   3. タブを閉じても他のタブ名が変わらず、次の作成は空いた番号を再利用する
#   4. prefix+N で名前指定のタブへフォーカスが移る
#   5. デタッチしてもセッションが残り、local-zellij で同セッションに再接続する
#   6. キー入力で操作できる（kitty形式・レガシー0x1C・Ctrl保持の数字）
#   7. 多重接続中に prefix+c を1回押してもタブは1つしか増えない
#   8. 「title:」パイプでバーのタブラベルが更新される（tmuxの#W相当）
#   9. 多重接続時、タブのフォーカスはクライアントごとに独立する
# =============================================================================
set -euo pipefail

USAGE="usage: local-zellij.sh <path-to-local-zellij> <config.kdl> <slots.kdl> <perm-seed.sh>"
SCRIPT="${1:?$USAGE}"
CONF="${2:?$USAGE}"
LAYOUT="${3:?$USAGE}"
SEED="${4:?$USAGE}"

# 前提の欠落は「タブが増えなかった」と区別がつかず false green になるため、
# テスト本体に入る前に落とす
command -v zellij >/dev/null || { echo "FAIL: zellij が見つからない"; exit 1; }
command -v script >/dev/null || { echo "FAIL: script(util-linux) が見つからない"; exit 1; }
[ -x "$SCRIPT" ] || { echo "FAIL: $SCRIPT が実行可能でない"; exit 1; }
[ -r "$CONF" ] || { echo "FAIL: $CONF が読めない"; exit 1; }
[ -r "$LAYOUT" ] || { echo "FAIL: $LAYOUT が読めない"; exit 1; }
[ -r "$SEED" ] || { echo "FAIL: $SEED が読めない"; exit 1; }

# プラグインの権限プロンプトは対話でしか承認できないため、設定とレイアウトから
# wasm のパス（zellij-slotsとzjstatus）を取り出して権限キャッシュを事前に与える
WASMS=$(grep -oh 'file:[^"]*\.wasm' "$CONF" "$LAYOUT" | sed 's/^file://' | sort -u)
[ -n "$WASMS" ] || { echo "FAIL: レイアウトからプラグインのパスを特定できない"; exit 1; }
while IFS= read -r wasm; do
  [ -r "$wasm" ] || { echo "FAIL: プラグイン $wasm が読めない"; exit 1; }
done <<< "$WASMS"
# 権限キャッシュの検証（シナリオ0）で、シード対象のエントリを名指しする
SLOTS_WASM=$(echo "$WASMS" | grep 'zellij-slots' | head -n1)
[ -n "$SLOTS_WASM" ] || { echo "FAIL: zellij-slotsのwasmを特定できない"; exit 1; }

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
  # 権限キャッシュは本番と同じ activation スクリプトで事前投入する。
  # 独自にシードすると、プラグインの要求権限とシード内容の乖離
  # （不足すると不可視の承認プロンプトでプラグインがブロックし、
  # バーが消える）をテストで検出できないため
  seed_permissions
}

# activation は home-manager の限られた PATH で実行され、裸のコマンド名は
# 落ちうる（awk: command not found の実績）。取りこぼしを検出できるよう
# PATH を空にし、activation 同様にエラーで即失敗させる
seed_permissions() {
  PATH="" "$BASH" -eu "$SEED"
}

# attach には端末が必要なので疑似端末上で起動する。
# script -c は文字列をシェルで解釈するため、パスは中でもクォートする
launch() {
  script -qec "'$SCRIPT'" /dev/null </dev/null >/dev/null 2>&1 &
  echo $!
}

# キー入力を送り込めるクライアントを起動する（stdinをFIFOから供給）。
# $1: FIFOのパス（この関数が作成する）。画面出力は "$1.out" に残す
# （typescriptファイルはブロックバッファされて直近フレームが欠けるため、
# stdoutリダイレクトで即時にキャプチャする）。呼び出し側で書き込み用fdを
# 開いたままにすること（閉じるとEOFでクライアントが終了するため）:
#   pid=$(launch_with_input "$WORK/in"); exec {fd}<>"$WORK/in"
# 呼び出し側の open は <> にする（> はFIFOの読み手が現れるまでブロックする）。
# この関数内でも FIFO の open (< "$1") は最後に書く。先に書くと、コマンド
# 置換のパイプを持ったままバックグラウンドのシェルがブロックし、呼び出し側
# とデッドロックする
launch_with_input() {
  mkfifo "$1"
  script -qec "'$SCRIPT'" /dev/null > "$1.out" 2>&1 < "$1" &
  echo $!
}

# そのクライアントの画面でハイライトされているタブ名（バーの反転表示）。
# フォーカスはクライアントごとに異なりうるので、セッション全体を見る
# focused_tab ではなく各クライアントの画面出力から読む。
# プラグインは "\e[47;30m" と書くが、Zellijは属性を組み直して出力するため
# 分割形式（"\e[30m\e[47m"）でも拾えるようにする
# $1: クライアントの出力ファイル（launch_with_input の "$FIFO.out"）
client_tab() {
  grep -aoP '\x1b\[(30m\x1b\[47|47;30)m \K[0-9]' "$1" | tail -1
}

# fdへキーのバイト列を書き込む（バックスラッシュエスケープを解釈）
# $1: fd番号  $2: バイト列
# 引数をフォーマット文字列にすると % で誤展開するため %b 固定にする
send_keys() {
  printf '%b' "$2" >&"$1"
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

# スロット操作（prefix+c / prefix+N）をキー入力で送る。
# CLIパイプではなく実際のキーバインドを通す。プラグインは操作の発信元を
# 「そのクライアントがprefixを押しているか」で見分けるため（シナリオ9）、
# クライアント不在のCLIパイプでは本番と同じ経路にならない
# $1: 書き込み用fd  $2: prefixに続けて送るキー
send_prefix() {
  send_keys "$1" "\x1b[92;5u$2"
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
# シナリオ0: 権限シードが不完全な既存エントリを補正し、他のエントリを保持する
# ---------------------------------------------------------------------------
# プラグインの要求権限が増えたとき（例: PR #126のReadCliPipes）、キャッシュに
# 同じwasmパスの古いエントリが残っていると権限不足で承認プロンプトが出て
# プラグインがブロックする。シードは既存エントリを検出してスキップするの
# ではなく、あるべき内容に補正しなければならない
echo "=== シナリオ0: 権限シードによる不完全エントリの補正 ==="
setup
PERMS_FILE="$XDG_CACHE_HOME/zellij/permissions.kdl"
fresh=$(cat "$PERMS_FILE")

# rebuildでwasmは変わらず要求権限だけが増えた状況を再現する
cat > "$PERMS_FILE" <<EOF
"$SLOTS_WASM" {
    ReadApplicationState
}
EOF
seed_permissions
[ "$(cat "$PERMS_FILE")" = "$fresh" ] \
  || fail "不完全なエントリが補正されなかった ($(cat "$PERMS_FILE"))"

# 別プラグインのエントリ（Zellij自身が承認時に書いたもの等）は壊さない
cat > "$PERMS_FILE" <<EOF
"/nix/store/other-plugin.wasm" {
    RunCommands
}
EOF
seed_permissions
grep -qF '"/nix/store/other-plugin.wasm"' "$PERMS_FILE" \
  || fail "他プラグインのエントリが消えた ($(cat "$PERMS_FILE"))"
grep -qF "\"$SLOTS_WASM\"" "$PERMS_FILE" \
  || fail "他エントリ保持時に自エントリが追加されなかった ($(cat "$PERMS_FILE"))"

# Zellij側の書式ゆらぎがあっても、自エントリの除去が行単位の完全一致に
# 失敗して他エントリを巻き込み削除したり、古いブロックを残したりしない
other_block='"/nix/store/other-plugin.wasm" {
    RunCommands
}'
expected="$other_block"$'\n'"$fresh"

# 閉じ括弧がインデントされている場合（除去の終了判定を誤ると後続の
# 他エントリがすべて消える）
{
  printf '"%s" {\n' "$SLOTS_WASM"
  printf '    ReadApplicationState\n'
  printf '  }\n'
  printf '%s\n' "$other_block"
} > "$PERMS_FILE"
seed_permissions
[ "$(cat "$PERMS_FILE")" = "$expected" ] \
  || fail "インデントされた閉じ括弧で補正が壊れた ($(cat "$PERMS_FILE"))"

# 開始行に末尾空白がある場合（開始判定を逃すと古いブロックが残る）
{
  printf '"%s" { \n' "$SLOTS_WASM"
  printf '    ReadApplicationState\n'
  printf '}\n'
  printf '%s\n' "$other_block"
} > "$PERMS_FILE"
seed_permissions
[ "$(cat "$PERMS_FILE")" = "$expected" ] \
  || fail "末尾空白つき開始行で古いブロックが残った ($(cat "$PERMS_FILE"))"
echo "PASS: 不完全なエントリを補正し、他のエントリを保持した"

# ---------------------------------------------------------------------------
# シナリオ1: 初回起動でセッション main が作られ、初期タブ名が「3」になる
# ---------------------------------------------------------------------------
echo "=== シナリオ1: 初回起動と初期スロット ==="
setup

pid1=$(launch_with_input "$WORK/in1")
exec 4<>"$WORK/in1"
wait_for_tabs 1 || fail "初回起動でセッションが作られなかった"
sleep 1
[ "$(sessions)" = "main" ] || fail "セッション名が main でない ($(sessions))"
[ "$(tab_names_csv)" = "3" ] || fail "初期タブ名が 3 でない ($(tab_names_csv))"
echo "PASS: セッション main、初期タブ 3"

# ---------------------------------------------------------------------------
# シナリオ2: prefix+c で優先順位どおり 4, 2, 8 のスロットにタブが増える
# ---------------------------------------------------------------------------
echo "=== シナリオ2: 優先順位による新規タブ作成 ==="
send_prefix 4 c
wait_for_tabs 2 || fail "prefix+c でタブが増えなかった"
send_prefix 4 c
wait_for_tabs 3 || fail "2回目の prefix+c でタブが増えなかった"
send_prefix 4 c
wait_for_tabs 4 || fail "3回目の prefix+c でタブが増えなかった"
sleep 1
[ "$(tab_names_csv)" = "3,4,2,8" ] \
  || fail "優先順位どおりに増えていない ($(tab_names_csv) 期待値: 3,4,2,8)"
echo "PASS: 3 → 4 → 2 → 8 の順にタブが増えた"

# ---------------------------------------------------------------------------
# シナリオ3: タブを閉じても他のタブ名が変わらず、次の作成は空き番号を再利用
# ---------------------------------------------------------------------------
# tmux の renumber-windows off 相当。位置は詰まってもスロット名は動かない
echo "=== シナリオ3: タブ削除後の番号安定性と再利用 ==="
send_prefix 4 4
sleep 1
[ "$(focused_tab)" = "4" ] || fail "prefix+4 でフォーカスが移らなかった ($(focused_tab))"
Z close-tab
sleep 1
[ "$(tab_names_csv)" = "3,2,8" ] \
  || fail "タブ4を閉じた後に名前が変わった ($(tab_names_csv) 期待値: 3,2,8)"
send_prefix 4 c
wait_for_tabs 4 || fail "削除後の prefix+c でタブが増えなかった"
sleep 1
[ "$(tab_names_csv)" = "3,2,8,4" ] \
  || fail "空きスロット 4 を再利用しなかった ($(tab_names_csv) 期待値: 3,2,8,4)"
echo "PASS: タブ名は安定し、空いた 4 を再利用した"

# ---------------------------------------------------------------------------
# シナリオ4: prefix+N で名前指定のタブへフォーカスが移る
# ---------------------------------------------------------------------------
echo "=== シナリオ4: 名前ジャンプ ==="
send_prefix 4 2
sleep 1
[ "$(focused_tab)" = "2" ] || fail "prefix+2 でフォーカスが移らなかった ($(focused_tab))"
send_prefix 4 3
sleep 1
[ "$(focused_tab)" = "3" ] || fail "prefix+3 でフォーカスが移らなかった ($(focused_tab))"
echo "PASS: prefix+数字でタブ間を移動できた"

# ---------------------------------------------------------------------------
# シナリオ5: デタッチ後もセッションが残り、local-zellij で再接続する
# ---------------------------------------------------------------------------
echo "=== シナリオ5: デタッチと再接続 ==="
kill "$pid1" 2>/dev/null || true
exec 4>&-
sleep 2
[ "$(sessions)" = "main" ] || fail "デタッチ後にセッションが残っていない ($(sessions))"

pid2=$(launch)
sleep 3
[ "$(sessions)" = "main" ] || fail "再接続でセッションが増減した ($(sessions))"
[ "$(tab_names_csv)" = "3,2,8,4" ] \
  || fail "再接続後にタブ構成が変わった ($(tab_names_csv))"
kill "$pid2" 2>/dev/null || true
sleep 2
echo "PASS: デタッチ後もセッションが残り、同セッションに再接続した"

# ---------------------------------------------------------------------------
# シナリオ6: キー入力で操作できる
# ---------------------------------------------------------------------------
# 端末は Ctrl+\ を2通りの形式で送ってくる:
#   - kitty keyboard protocol 対応端末（Alacritty）: CSI-u形式 \x1b[92;5u
#   - 非対応端末（VSCodeターミナル等）: レガシーの生バイト 0x1C
# Zellijが同梱するtermwizは 0x1C を「Ctrl \」に変換しないため、レガシー側は
# 生の制御文字 \u{1c} としてバインドしている。また数字はCtrlを押したまま
# （Ctrl+3 = \x1b[51;5u）でも離しても効くようにしている
echo "=== シナリオ6: キー入力（kitty形式・レガシー0x1C・Ctrl保持の数字） ==="
pidA=$(launch_with_input "$WORK/inA")
exec 8<>"$WORK/inA"
sleep 3

send_keys 8 '\x1b[92;5u2'          # CSI-u prefix + 素の数字
sleep 1.5
[ "$(focused_tab)" = "2" ] || fail "kitty形式の prefix+2 でフォーカスが移らなかった ($(focused_tab))"

send_keys 8 '\x1c\x1b[51;5u'       # レガシー prefix + Ctrl保持の3
sleep 1.5
[ "$(focused_tab)" = "3" ] || fail "レガシー0x1C prefix + Ctrl+3 でフォーカスが移らなかった ($(focused_tab))"

send_keys 8 '\x1b[92;5u\x1b[92;5u' # prefix 2回押しでlast-window相当
sleep 1.5
[ "$(focused_tab)" = "2" ] || fail "prefix 2回押しで直前のタブに戻らなかった ($(focused_tab))"
echo "PASS: kitty形式・レガシー0x1C・Ctrl保持の数字・2回押しが効いた"

# ---------------------------------------------------------------------------
# シナリオ7: 多重接続中に prefix+c を1回押してもタブは1つしか増えない
# ---------------------------------------------------------------------------
# config の load_plugins はクライアントのアタッチごとにプラグインインスタンスを
# 増やし、MessagePlugin のパイプは全インスタンスに配送される。作成が冪等で
# ないと、1回の prefix+c で接続クライアント数だけ同名タブが作られてしまう
echo "=== シナリオ7: 多重接続時のタブ作成の冪等性 ==="
pidB=$(launch_with_input "$WORK/inB")
exec 9<>"$WORK/inB"
sleep 3

send_prefix 8 c                    # クライアントAから prefix+c を1回
sleep 2
[ "$(tab_names_csv)" = "3,2,8,4,7" ] \
  || fail "2クライアント接続中の prefix+c でタブが重複した ($(tab_names_csv) 期待値: 3,2,8,4,7)"
kill "$pidA" "$pidB" 2>/dev/null || true
exec 8>&- 9>&-
echo "PASS: 多重接続中でもタブは1つだけ増えた"

# ---------------------------------------------------------------------------
# シナリオ8: 「title:」パイプでバーのタブラベルが更新される
# ---------------------------------------------------------------------------
# Zellijはペインタイトルの変更（OSC）だけではPaneUpdateを発行しないため、
# fishのフックがコマンドの開始・終了を「title:<pane_id>:<コマンド名>」の
# パイプで通知してくる。バーはこれを描画に反映する。
# プラグイン指定なしのパイプで、描画役のbarインスタンスに届くことも確認する。
# バーのインスタンスはクライアントごとに複製され、多重接続の残骸があると
# 配送が遅延するため、独立したセッションで検証する
echo "=== シナリオ8: titleパイプによるラベル更新 ==="
setup
pidC=$(launch_with_input "$WORK/inC")
exec 7<>"$WORK/inC"
sleep 4
# 最初のタブ（スロット3）の端末ペインはid=0（fishフックは$ZELLIJ_PANE_IDを使う）
timeout 10 zellij --session main pipe --name slots -- "title:0:TITLETEST" \
  || echo "WARN: titleパイプがタイムアウトした"
sleep 2
bar_line() {
  tr '\r' '\n' < "$WORK/inC.out" \
    | sed 's/\x1b\[[0-9;]*[a-zA-Z]//g' | grep -a 'main | ' | tail -1
}
case "$(bar_line)" in
  *"3:TITLETEST"*) : ;;
  *) fail "titleパイプがバーに反映されなかった ($(bar_line))" ;;
esac
kill "$pidC" 2>/dev/null || true
exec 7>&-
echo "PASS: titleパイプでタブ3のラベルが更新された"

# ---------------------------------------------------------------------------
# シナリオ9: 多重接続時、タブのフォーカスはクライアントごとに独立する
# ---------------------------------------------------------------------------
# 旧tmuxのグループセッションでは、複数端末が同じセッションを共有しつつ
# 別々のwindowを見られた。Zellijでも標準のタブ切り替えはクライアント単位で
# 動くが、プラグインのパイプはクライアントごとに複製された全インスタンスへ
# 配送されるため、素朴に実装すると全クライアントが同じタブへ移動してしまう。
# 押したクライアントだけが動くことを検証する
echo "=== シナリオ9: フォーカスのクライアント独立性 ==="
setup
pidD=$(launch_with_input "$WORK/inD")
exec 6<>"$WORK/inD"
sleep 4
send_prefix 6 c                    # D: スロット4を作成（Dはそこへ移動する）
sleep 2
pidE=$(launch_with_input "$WORK/inE")
exec 5<>"$WORK/inE"
sleep 4
[ "$(client_tab "$WORK/inD.out")" = "4" ] \
  || fail "Dが作成したスロット4にいない ($(client_tab "$WORK/inD.out"))"
[ "$(client_tab "$WORK/inE.out")" = "4" ] \
  || fail "接続直後のEが既存クライアントのタブを表示していない ($(client_tab "$WORK/inE.out"))"

send_prefix 5 3                    # E: スロット3へジャンプ
sleep 2
[ "$(client_tab "$WORK/inE.out")" = "3" ] \
  || fail "Eがスロット3へ移動しなかった ($(client_tab "$WORK/inE.out"))"
[ "$(client_tab "$WORK/inD.out")" = "4" ] \
  || fail "Eのジャンプに他クライアントDが引きずられた ($(client_tab "$WORK/inD.out"))"

send_prefix 6 c                    # D: 新規タブ（スロット2）を作成
sleep 2
[ "$(client_tab "$WORK/inD.out")" = "2" ] \
  || fail "Dが作成したスロット2へ移動しなかった ($(client_tab "$WORK/inD.out"))"
# タブ作成だけは全クライアントが新しいタブへ移動する。空きスロットの計算に
# 最新のタブ一覧が要るためバックグラウンドのインスタンスが担当するが、
# そのインスタンスはどのクライアントが押したのかを知る手段がない（Zellijの
# 制約。zellij.nix と zellij-slots/src/main.rs を参照）。Zellij側で
# 送信元クライアントが分かるようになったらここも独立させる
[ "$(client_tab "$WORK/inE.out")" = "2" ] \
  || fail "タブ作成後の状態が想定と違う ($(client_tab "$WORK/inE.out"))"

# プレフィックス操作の後はlockedモードに戻り、通常のキーがペインへ届く
send_keys 5 'echo LOCKEDBACK\n'
sleep 2
grep -aq 'LOCKEDBACK' "$WORK/inE.out" \
  || fail "操作後にlockedモードへ戻らずキー入力がペインに届かなかった"
kill "$pidD" "$pidE" 2>/dev/null || true
exec 6>&- 5>&-
echo "PASS: フォーカスはクライアントごとに独立した"

echo "すべてのシナリオが PASS"
