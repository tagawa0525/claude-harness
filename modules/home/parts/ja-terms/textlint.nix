# 崩れた日本語の語の検査（ADR-0014）に使う textlint と設定、コミットのメッセージの検査（ja-terms-message）。
# git.nix（pre-commit と commit-msg）と claude-hooks（git merge の本文を先に検査するルール。ADR-0050）が
# 同じものを store のパスで呼ぶ。
# 辞書は ./prh.yml。Markdown は textlint の組み込みのプラグインが、コードとテキストは @textlint/text が読む
# （コメントと文字列の日本語を含めて、ファイルの全体を見る）
{
  textlint,
  textlint-rule-prh,
  writeText,
  writeShellApplication,
  coreutils,
  gnused,
}:
let
  textlintWithRules = textlint.withPackages [ textlint-rule-prh ];
  codeExtensions = [
    "txt"
    "rs"
    "py"
    "sh"
    "nix"
    "toml"
    "yaml"
    "yml"
    "json"
  ];
  config = writeText "ja-terms-textlintrc.json" (
    builtins.toJSON {
      plugins."@textlint/text".extensions = map (ext: ".${ext}") codeExtensions;
      rules.prh.rulePaths = [ "${./prh.yml}" ];
    }
  );
  # コミットのメッセージ（標準入力）の検査。commit-msg と claude-hooks が同じ判定にするために共有する
  messageCheck = writeShellApplication {
    name = "ja-terms-message";
    runtimeInputs = [
      coreutils
      gnused
      textlintWithRules
    ];
    runtimeEnv.JA_TERMS_CONFIG = "${config}";
    text = builtins.readFile ./message.sh;
  };
in
{
  inherit config codeExtensions messageCheck;
  textlint = textlintWithRules;
}
