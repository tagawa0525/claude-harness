# shellcheck shell=bash
# hook のテストが共有する準備。各テストが source する。

# swap_home <dir>: HOME を一時ディレクトリに差し替える（利用者の git の設定と ~/.claude を見せない）。
# rustup / cargo は実環境のツールチェインを使う。RUSTUP_HOME を固定せずに HOME を差し替えると、rustup が既定の
# ツールチェインを見失い、rustfmt と cargo が「rustup could not choose a version」で失敗する
swap_home() {
  export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
  export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
  export HOME="$1"
  mkdir -p "$HOME"
}
