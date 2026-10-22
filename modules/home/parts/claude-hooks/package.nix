# claude-hooks: Claude Code の PreToolUse hook（Rust）。
# claude-code.nix から callPackage され、~/.claude/bin/claude-hooks に配置される。
# flake.nix の packages 出力（nix build .#claude-hooks）もここを参照する
{
  lib,
  rustPlatform,
  stdenvNoCC,
  makeBinaryWrapper,
  git,
  jq,
  callPackage,
}:
let
  gitCheckConfig = callPackage ../lib/git-check-config.nix { };
  # git merge の本文の語の検査（check-merge-body-ja-terms）が呼ぶ、commit-msg の hook と同じ検査（ja-terms-message）
  jaTermsTextlint = callPackage ../ja-terms/textlint.nix { };
  unwrapped = rustPlatform.buildRustPackage {
    pname = "claude-hooks-unwrapped";
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
    # ユニットテストをビルド時に実行する。pre_merge_check のテストは一時ディレクトリに git の
    # リポジトリを作るので、git を渡す（gh と hook の入出力を伴う結合テストは .claude/tests/hooks.sh）
    doCheck = true;
    nativeCheckInputs = [ git ];
    # sandbox に無い git の全体設定を環境の側から渡す（lib/git-check-config.nix）
    inherit (gitCheckConfig) preCheck;
    meta.mainProgram = "claude-hooks";
  };
in
# ja-terms-message の store のパスは、包む側で環境変数として固定する（ルールが実行時に読む）。Rust の
# ビルドに埋めると、辞書（prh.yml）を足すたびに crate の作り直しとテストが走る。hook は Claude Code が
# 起動するので、利用者の環境変数や PATH には頼らない。makeBinaryWrapper はシェルを挟まない
stdenvNoCC.mkDerivation {
  pname = "claude-hooks";
  inherit (unwrapped) version;
  dontUnpack = true;
  nativeBuildInputs = [ makeBinaryWrapper ];
  installPhase = ''
    runHook preInstall
    makeWrapper ${unwrapped}/bin/claude-hooks $out/bin/claude-hooks \
      --set CLAUDE_HOOKS_JA_TERMS_MESSAGE ${jaTermsTextlint.messageCheck}/bin/ja-terms-message
    runHook postInstall
  '';
  # 包んだ ja-terms-message と辞書で、git merge の本文の崩れた語を止めることを確かめる
  # （ユニットテストは偽の ja-terms-message を使う）
  doInstallCheck = true;
  nativeInstallCheckInputs = [
    git
    jq
  ];
  installCheckPhase = ''
    runHook preInstallCheck
    ${gitCheckConfig.preCheck}
    bash ${./tests/merge-ja-terms.sh} $out/bin/claude-hooks
    runHook postInstallCheck
  '';
  passthru = { inherit unwrapped; };
  meta.mainProgram = "claude-hooks";
}
