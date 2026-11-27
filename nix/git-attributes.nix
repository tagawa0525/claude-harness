# =============================================================================
# Git のグローバルの attributes（ADR-0105）
# =============================================================================
# ../config/git/attributes の行を programs.git.attributes に入れ、home-manager が ~/.config/git/attributes に置く
# （git が core.attributesFile の無いときに読む場所）。Windows では claude-setup git-attributes が同じファイルを置く。
# 置く場所の既存のファイルは、home-manager の checkLinkTargets が扱う（git hook のファイルと同じ）。
# 検証: nix/tests/git-attributes.sh
# =============================================================================
{ config, lib, ... }:

let
  placed = "${config.xdg.configFile."git/attributes".target}";
  # core.attributesFile が別の場所を指すと、git はそこだけを読み、置いたファイルは効かない
  attributesFile = config.programs.git.iniContent.core.attributesFile or null;
  readsPlaced = lib.elem attributesFile [
    null
    "${config.home.homeDirectory}/${placed}"
    "~/${placed}"
  ];
in
{
  programs.git.enable = true;
  # 先に置く: git は同じパスに当たる行のうち後の行を採るので、利用者が programs.git.attributes に足した行が勝つ
  programs.git.attributes = lib.mkBefore (
    lib.splitString "\n" (lib.removeSuffix "\n" (builtins.readFile ../config/git/attributes))
  );

  assertions = [
    {
      assertion = readsPlaced;
      message = "programs.git の core.attributesFile は ${toString attributesFile} を指しています。git はそこだけを読み、ハーネスが置く改行の規則（~/${placed}）が効きません。そのファイルの規則を programs.git.attributes に移して core.attributesFile を外してください";
    }
  ];
}
