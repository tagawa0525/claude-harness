# =============================================================================
# Git関連の設定
# =============================================================================
# Git, delta, GitHub CLI の設定（グローバルの hook は git-hooks.nix）
# =============================================================================
{ lib, pkgs, ... }:

{
  # ===========================================================================
  # Git設定
  # ===========================================================================
  programs.git = {
    enable = true;
    ignores = [
      # Claude Code
      "**/.claude/settings.local.json"

      # direnv
      ".envrc"
      ".direnv/"

      # Python
      "__pycache__/"
      "*.pyc"
      "*.pyo"
      ".venv/"
      ".mypy_cache/"
      ".pytest_cache/"
      ".ruff_cache/"

      # Node.js
      "node_modules/"

      # Rust
      "target/"
      "*.rs.bk"

      # Ruby
      "vendor/bundle/"
      ".bundle/"
      "*.gem"

      # エディタ/IDE
      ".idea/"
      "*.swp"
      "*.swo"
      "*~"

      # OS
      ".DS_Store"
      "Thumbs.db"

      # 環境変数（機密情報）
      ".env"
      ".env.local"
      ".env*.local"

      # ログ
      "*.log"
    ];
    settings = {
      user.name = "Hiroaki Tagawa";
      user.email = "tagawa0525@gmail.com";
      init.defaultBranch = "main"; # 新規リポジトリのデフォルトブランチ
      pull.rebase = true; # pull時にrebaseを使用（マージコミットを避ける）
      # 初回 push で上流を自動設定する。スキルの手順から「上流の有無で -u を付け分ける」分岐をなくす
      push.autoSetupRemote = true;
    };
  };

  # deltaでdiffを見やすく表示
  programs.delta = {
    enable = true;
    enableGitIntegration = true;
  };

  # ===========================================================================
  # GitHub CLI設定
  # ===========================================================================
  programs.gh = {
    enable = true;
    settings = {
      git_protocol = "https";
      prompt = "enabled";
    };
    gitCredentialHelper.enable = true;
  };

  # ===========================================================================
  # Nix の GitHub access-tokens 自動設定
  # ===========================================================================
  # gh auth のトークンを $HOME/.config/nix/nix.conf に書き出し、
  # nix flake update 時の rate limit (60回/時→5000回/時) を回避する
  home.activation.nixGithubToken = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    if ${pkgs.gh}/bin/gh auth status &>/dev/null; then
      TOKEN=$(${pkgs.gh}/bin/gh auth token)
      if [ -n "$TOKEN" ]; then
        $DRY_RUN_CMD mkdir -p "$HOME/.config/nix"
        if [ -z "''${DRY_RUN_CMD:-}" ]; then
          tmp_conf="$(mktemp "$HOME/.config/nix/nix.conf.XXXXXX")"
          if [ -f "$HOME/.config/nix/nix.conf" ]; then
            grep -v '^access-tokens[[:space:]]*=' "$HOME/.config/nix/nix.conf" > "$tmp_conf" || [ $? -eq 1 ]
          fi
          echo "access-tokens = github.com=$TOKEN" >> "$tmp_conf"
          chmod 600 "$tmp_conf"
          mv "$tmp_conf" "$HOME/.config/nix/nix.conf"
        fi
      fi
    fi
  '';
}
