# history-rewrite: 履歴の中の崩れた語を辞書で直し、番号を commit-refs で付け直す CLI（ADR-0045、ADR-0088）。
# git-hooks.nix から callPackage され、PATH に入る。flake.nix の packages 出力（nix build .#history-rewrite）と
# checks もここを参照する
{
  lib,
  rustPlatform,
  makeBinaryWrapper,
  git,
  callPackage,
  commit-refs ? callPackage ../commit-refs/package.nix { },
}:
let
  gitCheckConfig = callPackage ../lib/git-check-config.nix { };
in
rustPlatform.buildRustPackage {
  pname = "history-rewrite";
  version = "0.1.0";
  # パス依存の ../ja-terms（辞書と prh の当て方。既定の辞書の prh.yml をビルドに埋める）、../fix-markdown-lint
  # （表の揃え方）、../commit-refs（オブジェクトの読み方）もソースに入れ、history-rewrite の中でビルドする
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.difference (lib.fileset.unions [
      ./.
      ../ja-terms/Cargo.toml
      ../ja-terms/src
      ../ja-terms/prh.yml
      ../fix-markdown-lint/Cargo.toml
      ../fix-markdown-lint/src
      ../commit-refs/Cargo.toml
      ../commit-refs/src
    ]) ./package.nix;
  };
  buildAndTestSubdir = "history-rewrite";
  cargoRoot = "history-rewrite";
  cargoLock.lockFile = ./Cargo.lock;
  # テストは一時のリポジトリで git を実行し、run の第 2 段と sync-worktrees は PATH の commit-refs を呼ぶ
  nativeCheckInputs = [
    git
    commit-refs
  ];
  # sandbox に無い git の全体設定を環境の側から渡す（lib/git-check-config.nix）
  inherit (gitCheckConfig) preCheck;
  # 呼ぶ git と commit-refs は、利用者の PATH（無いか古い版）に左右されないよう store のものを先に置く（cargo install する
  # Windows では PATH のものを使う）
  nativeBuildInputs = [ makeBinaryWrapper ];
  postInstall = ''
    wrapProgram $out/bin/history-rewrite --prefix PATH : ${
      lib.makeBinPath [
        git
        commit-refs
      ]
    }
  '';
  meta.mainProgram = "history-rewrite";
}
