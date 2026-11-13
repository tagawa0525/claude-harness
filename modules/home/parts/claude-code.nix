# =============================================================================
# Claude Code の設定
# =============================================================================
# グローバル CLAUDE.md/skills/commands/scripts の同期、PreToolUse hook、
# （./claude-hooks、Rust 製 1 バイナリ）の配置、UserPromptSubmit hook（現在時刻）と Stop hook（自動メモリの退避）の登録、settings.json 管理
# （cc-bar 統合は ./modules/cc-bar.nix に集約）
# =============================================================================
# flake.nix が lib.modules.importApply で 1 つ目の引数を束ねる。モジュールの引数（_module.args、specialArgs）で
# 受けないのは、利用する側が同じ名前を specialArgs で渡すと、黙ってそちらが勝つため
{
  claudeCodeSource, # .claude/ を ~/.claude に同期する元（この flake の self）
  mattpocock-skills,
  lsp-det,
}:
{
  pkgs,
  lib,
  ...
}:

let
  # 外部リポジトリ由来のスキル。出所は flake input（rev は flake.lock）で記録し、
  # 更新は `nix flake update <input>` → rebuild で追従する。
  # デプロイ先ディレクトリは丸ごと上流のコピーとして扱う（ローカル編集は上書きされる）
  externalSkills = [
    {
      name = "grilling";
      src = "${mattpocock-skills}/skills/productivity/grilling";
    }
    {
      name = "grill-me";
      src = "${mattpocock-skills}/skills/productivity/grill-me";
    }
  ];

  # ハーネスを Claude Code の設定に配る設置のコマンド（ADR-0084）。claude-sync と activation が sync を呼ぶ
  claude-setup = pkgs.callPackage ./claude-setup/package.nix { };
  claudeSync = pkgs.writeShellScriptBin "claude-sync" ''exec ${claude-setup}/bin/claude-setup sync "$@"'';
  # PreToolUse hook。10 本あった bash hook を Rust 製 1 バイナリ（./claude-hooks）に統合し、
  # settings.json には 1 件だけ登録する。Bash ツールの呼び出しごとに 1 プロセスで全ルールを評価する。
  # バイナリは store パスではなく固定パス ~/.claude/bin/claude-hooks 経由で参照する
  # （settings.json に store パスを書くと世代ごとに書き換わる。zellij.nix のプラグインと同じ理由）
  claude-hooks = pkgs.callPackage ./claude-hooks/package.nix { };
  claudeHooksBinRel = ".claude/bin/claude-hooks";
  # 崩れた日本語の語の検査（ADR-0074、ADR-0085）。/retro の memory-index.sh が `ja-terms check` で自動メモリに使う。
  # スキルのスクリプトは claude-sync が store を経ずにコピーするので store のパスを埋められず、PATH に頼らない
  # よう claude-hooks と同じ固定パスに置く
  ja-terms = pkgs.callPackage ./ja-terms/package.nix { };
  # Stop の hook（応答の終わりごと）。自動メモリを ~/.claude の git の ref refs/memory-backup にコミットする（ADR-0076）。
  # claude-hooks と同じく、settings.json には store のパスでなく固定パスを書く
  claudeMemoryBackup = pkgs.writeShellApplication {
    name = "claude-memory-backup";
    runtimeInputs = with pkgs; [
      coreutils
      git
      jq
      util-linux # flock
    ];
    text = builtins.readFile ../scripts/claude-memory-backup.sh;
  };
  claudeMemoryBackupBinRel = ".claude/bin/claude-memory-backup";
  # 旧 bash hook のファイル名。settings.json の登録と ~/.claude/hooks の残骸を掃除するために残す
  legacyHookFiles = [
    "block-main-commit.sh"
    "block-secret-commit.sh"
    "guard-gh-api.sh"
    "guard-gh-run-rerun.sh"
    "guard-git-add.sh"
    "guard-git-push.sh"
    "pre-merge-check.sh"
    "pre-pr-create-check.sh"
    "require-background-wait.sh"
    "warn-large-commit.sh"
  ];

in
{
  # ===========================================================================
  # パッケージ（Claude Code と Claude Desktop の本体は claude-apps.nix）
  # ===========================================================================
  home.packages =
    with pkgs;
    [
      gitleaks # 手で検査するため（git の pre-commit は store のパスで呼ぶ。ADR-0015）
      pyright # lsp-det のドッグフーディングの .lsp.json が pyright-langserver を PATH に求める（公式の pyright-lsp も同じ）
    ]
    ++ [ lsp-det.packages.${pkgs.stdenv.hostPlatform.system}.default ];

  # lsp-det のドッグフーディングのプラグイン（flake input の固定 rev の dogfood/claude-plugin）。
  # skills 直下のプラグインは Claude Code が毎回読む（lsp-det-dogfood@skills-dir）。
  # 上流は .lsp.json が PATH に求める（rust-analyzer は各リポジトリの devShell、nixd はシステム、pyright は上）
  home.file.".claude/skills/lsp-det-dogfood".source = "${lsp-det}/dogfood/claude-plugin";

  # PreToolUse hook バイナリ（settings.json はこの固定パスを参照する）
  home.file.${claudeHooksBinRel}.source = "${claude-hooks}/bin/claude-hooks";
  home.file.".claude/bin/ja-terms".source = "${ja-terms}/bin/ja-terms";
  home.file.${claudeMemoryBackupBinRel}.source = "${claudeMemoryBackup}/bin/claude-memory-backup";

  # .claude（CLAUDE.md/commands/skills/scripts）を ~/.claude に手動同期するコマンド
  # nixos-rebuild を待たずにスキル変更を反映する（bash/fish 共通で使用可）。中身は claude-setup sync（ADR-0084）
  home.file.".local/bin/claude-sync".source = "${claudeSync}/bin/claude-sync";

  # ===========================================================================
  # アクティベーションスクリプト
  # ===========================================================================

  # Claude Code グローバル設定の同期
  # flakeソースの .claude を ~/.claude に同期（ポリシーは ./claude-setup/src/sync.rs を参照）
  home.activation.claudeCodeSetup = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    CLAUDE_DIR="$HOME/.claude"

    # .claude ディレクトリを作成
    mkdir -p "$CLAUDE_DIR"

    # CLAUDE.md/commands/skills/scripts の同期（claude-sync コマンドと共通実装の claude-setup sync）
    $DRY_RUN_CMD ${claude-setup}/bin/claude-setup sync "${claudeCodeSource}"


    # 外部スキルのデプロイ（externalSkills）。上流のコピーなので --delete で完全一致させる。
    # claude-sync コマンドは flake input を知らないため、ここ（rebuild）でのみ同期される。
    # --checksum は、store のソースと -a でコピーした先がどちらも mtime 1 になり、既定の判定
    # （サイズと mtime）では同じサイズの上流の変更を飛ばすため（claude-setup sync も同じ理由で中身を比べる。比べた案は nixfiles の issue 566）
    ${lib.concatMapStringsSep "\n" (s: ''
      $DRY_RUN_CMD mkdir -p "$CLAUDE_DIR/skills/${s.name}"
      $DRY_RUN_CMD ${pkgs.rsync}/bin/rsync -a --checksum --delete --chmod=u+w "${s.src}/" "$CLAUDE_DIR/skills/${s.name}/"
    '') externalSkills}

    # 静的な設定（./claude-settings.json）と hook を settings.json に反映する（ADR-0084。規則と登録の形、timeout は
    # claude-setup の src/settings.rs）。静的な設定の enabledPlugins の rust-analyzer-lsp を false にするのは、lsp-det の
    # ドッグフーディング（~/.claude/skills/lsp-det-dogfood）と .rs を取り合うため（同じ拡張子は先に登録された方が勝ち、
    # skills とマーケットプレイスの順序は文書化されていない）。permissions.allow の design-records と commit-refs は
    # git-hooks.nix が PATH に入れる設計の記録の CLI（adr スキル、git-merge スキル、branch-topics.sh が呼ぶ）、~/.claude の下の
    # スクリプトは gh-pr-review、language-checks、retro のスキルと共有のスクリプト（PR のレビュー待ち、git と gh の
    # 決まった手順）
    $DRY_RUN_CMD ${claude-setup}/bin/claude-setup settings --static ${./claude-settings.json} \
      --pre-tool-use "$HOME/${claudeHooksBinRel}" \
      --stop "$HOME/${claudeMemoryBackupBinRel}" \
      ${lib.concatMapStringsSep " " (f: "--legacy ${f}") legacyHookFiles}

    # 旧 bash hook の残骸を消す（claude-sync は --delete を使わないため残り続ける）。
    # activation は限られた PATH で実行されるため、外部コマンドはストアパスで参照する
    ${lib.concatMapStringsSep "\n" (
      f: ''$DRY_RUN_CMD ${pkgs.coreutils}/bin/rm -f "$CLAUDE_DIR/hooks/${f}"''
    ) legacyHookFiles}
    $DRY_RUN_CMD ${pkgs.coreutils}/bin/rm -f "$CLAUDE_DIR/hooks/lib/heredoc.sh"
    $DRY_RUN_CMD ${pkgs.coreutils}/bin/rmdir --ignore-fail-on-non-empty "$CLAUDE_DIR/hooks/lib" "$CLAUDE_DIR/hooks" 2>/dev/null || true
  '';
}
