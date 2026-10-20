# history-rewrite: 履歴の中の崩れた語を辞書で直し、番号を commit-refs で付け直す CLI（ADR-0045）。
# git.nix から callPackage され、PATH に入る。flake.nix の packages 出力（nix build .#history-rewrite）と
# checks もここを参照する
{
  lib,
  stdenvNoCC,
  makeWrapper,
  python3,
  git,
  textlint,
  textlint-rule-prh,
  callPackage,
  commit-refs ? callPackage ../commit-refs/package.nix { },
}:
let
  python = python3.withPackages (ps: [
    ps.git-filter-repo
    ps.pyyaml
    ps.regex
  ]);
  gitCheckConfig = callPackage ../lib/git-check-config.nix { };
  # 既定の辞書は、pre-commit と commit-msg の ja-terms の検査（ADR-0014）と同じもの
  dictionary = ../ja-terms/prh.yml;
  # 表の揃え直しは、run-checks.sh の Markdown の自動修正と同じツール
  fixMarkdownLint = ../../../../.claude/skills/language-checks/scripts/fix-markdown-lint.py;
  env = {
    HISTORY_REWRITE_DICTIONARY = "${dictionary}";
    HISTORY_REWRITE_FIX_MARKDOWN_LINT = "${fixMarkdownLint}";
  };
in
stdenvNoCC.mkDerivation {
  pname = "history-rewrite";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.fileFilter (file: file.hasExt "py") ./.;
  };
  nativeBuildInputs = [ makeWrapper ];
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    mkdir -p $out/libexec/history-rewrite $out/bin
    cp prh_rules.py history_rewrite.py $out/libexec/history-rewrite/
    makeWrapper ${python}/bin/python3 $out/bin/history-rewrite \
      --add-flags $out/libexec/history-rewrite/history_rewrite.py \
      --prefix PATH : ${
        lib.makeBinPath [
          git
          commit-refs
        ]
      } \
      ${lib.concatStringsSep " " (lib.mapAttrsToList (k: v: "--set ${k} ${v}") env)}
    runHook postInstall
  '';
  # テストは包んだ $out/bin/history-rewrite を実行する。textlint は、書き換えた結果が hook の検査を通るかを見る
  doInstallCheck = true;
  nativeInstallCheckInputs = [
    git
    (textlint.withPackages [ textlint-rule-prh ])
  ];
  installCheckPhase = ''
    runHook preInstallCheck
    ${gitCheckConfig.preCheck}
    export PATH=$out/bin:${commit-refs}/bin:$PATH
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (k: v: "export ${k}=${v}") env)}
    ${python}/bin/python3 -m unittest discover -s . -p 'test_*.py' -v
    runHook postInstallCheck
  '';
  meta.mainProgram = "history-rewrite";
}
