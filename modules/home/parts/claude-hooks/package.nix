# claude-hooks: Claude Code の PreToolUse hook（Rust）。
# claude-code.nix から callPackage され、~/.claude/bin/claude-hooks に配置される。git-hooks.nix も callPackage し、
# git hook のファイルが store のパスで呼ぶ（git-hook のサブコマンド。ADR-0086）。
# flake.nix の packages 出力（nix build .#claude-hooks）もここを参照する
{
  lib,
  rustPlatform,
  stdenvNoCC,
  makeBinaryWrapper,
  git,
  jq,
  gitleaks,
  callPackage,
}:
let
  gitCheckConfig = callPackage ../lib/git-check-config.nix { };
  # git merge と git commit のメッセージの検査（check-merge-body-ja-terms、check-commit-message）が呼ぶ
  # `ja-terms commit-msg`（commit-msg の hook と同じ検査）と、git merge の本文の語だけを見る `ja-terms message`（ADR-0085）
  jaTerms = callPackage ../ja-terms/package.nix { };
  # post-rewrite の git hook（git-hook post-rewrite）が呼ぶ `commit-refs remap`（ADR-0012、ADR-0086）
  commitRefs = callPackage ../commit-refs/package.nix { };
  unwrapped = rustPlatform.buildRustPackage {
    pname = "claude-hooks-unwrapped";
    version = "0.1.0";
    # パス依存の ../design-records（ADR の形）と ../fix-markdown-lint（Markdown の直し）もソースに入れ、
    # claude-hooks の中でビルドする。包む側のファイル（この package.nix と installCheckPhase のスクリプト）は除き、
    # それらの変更で crate を作り直さない
    src = lib.fileset.toSource {
      root = ../.;
      fileset =
        lib.fileset.difference
          (lib.fileset.unions [
            ./.
            ../design-records
            ../fix-markdown-lint
          ])
          (
            lib.fileset.unions [
              ./package.nix
              ./tests/merge-ja-terms.sh
              ./tests/commit-message.sh
              ./tests/review-rounds.sh
            ]
          );
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
# ja-terms の store のパスは、包む側で環境変数として固定する（ルールが実行時に読む）。claude-hooks の Rust のビルドに
# 埋めると、辞書（prh.yml）や commit-msg の規則を変えるたびに claude-hooks の作り直しとテストが走る。hook は Claude Code が
# 起動するので、利用者の環境変数や PATH には頼らない。makeBinaryWrapper はシェルを挟まない
# git は PATH の後ろに足す（利用者の git を先に使い、PATH に git の無い環境から起動した Claude Code でも、Stop の hook の
# 退避と PreToolUse のルールが git を起動できる）
stdenvNoCC.mkDerivation {
  pname = "claude-hooks";
  inherit (unwrapped) version;
  dontUnpack = true;
  nativeBuildInputs = [ makeBinaryWrapper ];
  installPhase = ''
    runHook preInstall
    makeWrapper ${unwrapped}/bin/claude-hooks $out/bin/claude-hooks \
      --set CLAUDE_HOOKS_JA_TERMS ${jaTerms}/bin/ja-terms \
      --set CLAUDE_HOOKS_COMMIT_REFS ${commitRefs}/bin/commit-refs \
      --set CLAUDE_HOOKS_GITLEAKS ${gitleaks}/bin/gitleaks \
      --suffix PATH : ${git}/bin
    runHook postInstall
  '';
  # 包んだ ja-terms と辞書で git merge の本文の崩れた語を、同じ ja-terms で git commit の
  # 件名を止めることと、/code-review の周を数えることを確かめる（ユニットテストは偽の検査を使う）
  doInstallCheck = true;
  nativeInstallCheckInputs = [
    git
    jq
  ];
  installCheckPhase = ''
    runHook preInstallCheck
    ${gitCheckConfig.preCheck}
    bash ${./tests/merge-ja-terms.sh} $out/bin/claude-hooks
    bash ${./tests/commit-message.sh} $out/bin/claude-hooks
    bash ${./tests/review-rounds.sh} $out/bin/claude-hooks
    runHook postInstallCheck
  '';
  meta.mainProgram = "claude-hooks";
}
