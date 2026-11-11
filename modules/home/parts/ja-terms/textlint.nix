# 崩れた日本語の語の検査（ADR-0014）に使う textlint と設定、コミットのメッセージの検査（ja-terms-message）と、
# それを含むコミットの規約の検査（commit-msg-check）。git-hooks.nix（pre-commit と commit-msg）と claude-hooks（git merge の
# 本文の語を先に検査するルール。ADR-0050。git commit の本文を先に検査するルール。ADR-0073）が同じものを store の
# パスで呼ぶ。ファイルの全体の検査（ja-terms-files）は、claude-code.nix が ~/.claude/bin に置き、/retro の
# memory-index.sh が自動メモリに使う（ADR-0074）。
# 辞書は ./prh.yml。Markdown は textlint の組み込みのプラグインが、コードとテキストは @textlint/text が読む
# （コメントと文字列の日本語を含めて、ファイルの全体を見る）
{
  textlint,
  textlint-rule-prh,
  writeText,
  writeShellApplication,
  coreutils,
  gnused,
  gnugrep,
  gawk,
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
  # 引数のファイルの全体の検査。git で追跡しない文章（自動メモリ）を pre-commit と同じ辞書と設定で見る
  filesCheck = writeShellApplication {
    name = "ja-terms-files";
    runtimeInputs = [
      coreutils
      gawk
      gnugrep
      textlintWithRules
    ];
    runtimeEnv = {
      JA_TERMS_CONFIG = "${config}";
      JA_TERMS_EXTENSIONS = builtins.concatStringsSep "\n" ([ "md" ] ++ codeExtensions);
    };
    text = builtins.readFile ./files.sh;
  };
  # コミットのメッセージ（ファイル）の規約の検査（件名と語）。commit-msg と claude-hooks が同じ判定にするために共有する
  commitMsgCheck = writeShellApplication {
    name = "commit-msg-check";
    runtimeInputs = [
      coreutils
      gnused
      gnugrep
      messageCheck
    ];
    text = builtins.readFile ./commit-msg-check.sh;
  };
in
{
  inherit
    config
    codeExtensions
    messageCheck
    commitMsgCheck
    filesCheck
    ;
  textlint = textlintWithRules;
}
