# =============================================================================
# Claude Code のハーネス
# =============================================================================
# グローバルの指示、スキル、スクリプト（claude/）、PreToolUse hook と git の hook、それらが使う CLI。
# 利用する側（NixOS と home-manager の構成）は、input に足して homeManagerModules.default を import する。
# input の nixpkgs と home-manager は、利用する側で follows にして版を揃える。
# =============================================================================
{
  description = "Claude Code harness: global instructions, skills, hooks and their CLIs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    # テスト用の構成（homeConfigurations.test）を評価するためだけに使う。利用する側の home-manager には触れない
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # mattpocock/skills: Claude Code 向けスキル集。grilling / grill-me を ~/.claude/skills にデプロイする
    # （対象は nix/claude-code.nix の externalSkills）。flake ではないので出所と rev の記録の
    # ためだけに input にする
    mattpocock-skills = {
      url = "github:mattpocock/skills";
      flake = false;
    };

    # lsp-det: 言語サーバーの状態を示す透過プロキシ（自作）。packages.default を Claude Code の LSP の経路に
    # 挟んでドッグフーディングする（claude-code.nix）
    lsp-det = {
      url = "github:tagawa0525/lsp-det";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      mattpocock-skills,
      lsp-det,
    }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      # ~/.claude の中身と settings.json、PreToolUse hook、git のグローバルの hook。
      # Claude Code の本体は入れない（版は利用する側の構成が決める）
      homeManagerModules.default = {
        _file = ./flake.nix;
        key = "claude-harness#homeManagerModules.default"; # 2 か所から import されても 1 つにまとまる
        imports = [
          (nixpkgs.lib.modules.importApply ./nix/claude-code.nix {
            claudeCodeSource = self;
            inherit mattpocock-skills lsp-det;
          })
          ./nix/git-hooks.nix
        ];
      };

      # テストが hook のファイルをビルドするための構成（nix/tests/）。実際のホームには使わない
      homeConfigurations.test = home-manager.lib.homeManagerConfiguration {
        inherit pkgs;
        modules = [
          self.homeManagerModules.default
          {
            home.username = "test";
            home.homeDirectory = "/home/test";
            home.stateVersion = "25.11";
          }
        ];
      };

      packages.${system} = {
        # Claude Code の PreToolUse hook（nix/claude-code.nix が home に配置する）。
        # nix build .#claude-hooks で単体ビルドし、テストから CLAUDE_HOOKS_BIN で参照する
        claude-hooks = pkgs.callPackage ./crates/claude-hooks/package.nix { };
        # 文書の中のコミットの番号の付け直しの CLI（post-rewrite の git hook の中身、claude-hooks の src/git_hook.rs が呼ぶ）
        commit-refs = pkgs.callPackage ./crates/commit-refs/package.nix { };
        # 履歴の中の崩れた語を直し、番号を付け直す CLI（nix/git-hooks.nix が PATH に入れる）
        history-rewrite = pkgs.callPackage ./crates/history-rewrite/package.nix {
          inherit (self.packages.${system}) commit-refs;
        };
        # 設計の記録（ADR）のテンプレートと検査の CLI（nix/git-hooks.nix が PATH に入れる）
        design-records = pkgs.callPackage ./crates/design-records/package.nix { };
        # 崩れた日本語の語の検査の crate と CLI（ADR-0085。pre-commit、commit-msg、claude-hooks、/retro が呼ぶ）
        ja-terms = pkgs.callPackage ./crates/ja-terms/package.nix { };
        # ハーネスを Claude Code の設定に配る設置のコマンド（ADR-0084。claude-sync と activation が呼ぶ）
        claude-setup = pkgs.callPackage ./crates/claude-setup/package.nix { };
      };

      # nix flake check は packages を評価するだけで checkPhase（cargo test）を実行しない。
      # サンドボックスでだけ落ちるテスト（PATH に git が無い等）を見つけるため、テストのあるパッケージを checks に入れる
      # モジュール（claude-code.nix、git-hooks.nix）の評価の誤りも止めるため、テスト用の構成の activation もビルドする
      checks.${system} = {
        inherit (self.packages.${system})
          claude-hooks
          claude-setup
          commit-refs
          design-records
          history-rewrite
          ja-terms
          ;
        home-test = self.homeConfigurations.test.activationPackage;
      };
    };
}
