# git を使うテストを持つパッケージの check phase に渡す、git の全体設定。
# nix の sandbox には ~/.gitconfig が無く、init.defaultBranch が main にならない等、手元の cargo test と
# 結果が変わる。環境の側で $HOME/.gitconfig に固定の設定を置き、パッケージごとの対処を要らなくする。
# GIT_CONFIG_GLOBAL にしないのは、テストが hook から受け継ぐ GIT_ で始まる環境変数を外す（isolate）ため、
# GIT_ の変数は子の git に届かないから。HOME は外されない
# 使い方: `gitCheckConfig = callPackage ../lib/git-check-config.nix { };` のあと
# `inherit (gitCheckConfig) preCheck;`（nativeCheckInputs の git は各パッケージが持つ）
{ writeText }:
let
  gitconfig = writeText "git-check-gitconfig" ''
    [init]
    	defaultBranch = main
    [user]
    	name = t
    	email = t@t
    [commit]
    	gpgsign = false
    [tag]
    	gpgsign = false
  '';
in
{
  preCheck = ''
    export HOME=$TMPDIR
    ln -s ${gitconfig} $HOME/.gitconfig
  '';
}
