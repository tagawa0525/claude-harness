# claude-hooks: Claude Code の PreToolUse hook（Rust）。
# claude-code.nix から callPackage され、~/.claude/bin/claude-hooks に配置される。
# flake.nix の packages 出力（nix build .#claude-hooks）もここを参照する
{
  lib,
  rustPlatform,
  git,
  jq,
  callPackage,
}:
let
  gitCheckConfig = callPackage ../lib/git-check-config.nix { };
  # git merge の本文の語の検査（check-merge-body-ja-terms）が呼ぶ、commit-msg の hook と同じ検査（ja-terms-message）
  jaTermsTextlint = callPackage ../ja-terms/textlint.nix { };
in
rustPlatform.buildRustPackage {
  pname = "claude-hooks";
  version = "0.1.0";
  # パス依存の ../design-records（ADR の形）もソースに入れ、claude-hooks の中でビルドする
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ./.
      ../design-records
    ];
  };
  buildAndTestSubdir = "claude-hooks";
  cargoRoot = "claude-hooks";
  cargoLock.lockFile = ./Cargo.lock;
  # store のパスをバイナリに埋める（ルールが option_env! で読む）。hook は Claude Code が起動するので、
  # 実行時の環境変数や PATH には頼らない
  env.CLAUDE_HOOKS_JA_TERMS_MESSAGE = "${jaTermsTextlint.messageCheck}/bin/ja-terms-message";
  # ユニットテストをビルド時に実行する。pre_merge_check のテストは一時ディレクトリに git の
  # リポジトリを作るので、git を渡す（gh と hook の入出力を伴う結合テストは .claude/tests/hooks.sh）
  doCheck = true;
  nativeCheckInputs = [ git ];
  # sandbox に無い git の全体設定を環境の側から渡す（lib/git-check-config.nix）
  inherit (gitCheckConfig) preCheck;
  # 埋めた ja-terms-message と辞書で、ビルドしたバイナリが git merge の本文の崩れた語を止めることを確かめる
  # （ユニットテストは偽の ja-terms-message を使う）
  doInstallCheck = true;
  nativeInstallCheckInputs = [
    git
    jq
  ];
  installCheckPhase = ''
    runHook preInstallCheck
    bash ${./tests/merge-ja-terms.sh} $out/bin/claude-hooks
    runHook postInstallCheck
  '';
  meta.mainProgram = "claude-hooks";
}
