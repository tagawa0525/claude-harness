# 崩れた日本語の語の検査（ADR-0014）に使う textlint と設定。git.nix（pre-commit と commit-msg）と
# claude-hooks（git merge の本文を数秒で先に検査するルール）が同じものを store のパスで呼ぶ。
# 辞書は ./prh.yml。Markdown は textlint の組み込みのプラグインが、コードとテキストは @textlint/text が読む
# （コメントと文字列の日本語を含めて、ファイルの全体を見る）
{
  textlint,
  textlint-rule-prh,
  writeText,
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
in
{
  inherit config codeExtensions;
  textlint = textlintWithRules;
  # 設定を付けた呼び出し。引数は呼ぶ側が足す
  command = "${textlintWithRules}/bin/textlint --config ${config}";
}
