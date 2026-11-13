# shellcheck shell=bash
# cargo-bin.sh — リポジトリの crate をビルドしてテストで使う（source して使う）
#
# cargo_bin <crate のディレクトリ> <バイナリの名前>
#   crate をビルドし、バイナリのパスを出す。見つからなければ理由を標準エラーに出して 1 を返す。
#   lib.sh が HOME を差し替える前に呼ぶ（~/.cargo/config.toml の sccache 等を使うため）。バイナリの場所は cargo に聞く
#   （~/.cargo/config.toml の target-dir や CARGO_TARGET_DIR で target/ の外に出る）
cargo_bin() {
  local built bin
  built=$(cargo build -q --message-format=json --manifest-path "$1/Cargo.toml") || return 1
  bin=$(jq -r --arg name "$2" 'select(.reason == "compiler-artifact" and .target.name == $name and .executable != null) | .executable' <<<"$built")
  if [[ ! -x "$bin" ]]; then
    echo "ERROR: ビルドした $2 が見つかりません: '$bin'" >&2
    return 1
  fi
  printf '%s\n' "$bin"
}
