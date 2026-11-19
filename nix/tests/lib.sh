# shellcheck shell=bash
# hook のテストが共有する準備。各テストが source する。

# 本物の git hook が呼ぶ ja-terms は、共通の辞書を claude-setup sync が配ったホームの下から読む（ADR-0097）。テストは
# 配らないので、ソースの辞書を渡す（渡さないと、どのコミットも「共通の辞書が無い」で止まり、止まることを確かめるテストが
# 検査によらず通る）。配る経路を確かめる ja-terms.sh は、source の後に外す
JA_TERMS_COMMON_DICTIONARY="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/crates/ja-terms/prh.yml"
export JA_TERMS_COMMON_DICTIONARY

# swap_home <dir>: HOME を一時ディレクトリに差し替える（利用者の git の設定と ~/.claude を見せない）。
# rustup / cargo は実環境のツールチェインを使う。RUSTUP_HOME を固定せずに HOME を差し替えると、rustup が既定の
# ツールチェインを見失い、rustfmt と cargo が「rustup could not choose a version」で失敗する
swap_home() {
  export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
  export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
  export HOME="$1"
  mkdir -p "$HOME"
}
