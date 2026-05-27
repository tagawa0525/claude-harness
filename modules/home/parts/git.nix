# =============================================================================
# Git関連の設定
# =============================================================================
# Git, Git Hooks, delta, GitHub CLI の設定
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
      core.hooksPath = "~/.config/git/hooks"; # グローバルhooksを使用
    };
  };

  # ===========================================================================
  # Git Hooks（グローバル）
  # ===========================================================================
  # プロジェクトローカルの .git/hooks/ があれば優先、なければデフォルトチェック
  xdg.configFile."git/hooks/pre-commit" = {
    executable = true;
    text = ''
      #!/usr/bin/env bash
      set -euo pipefail

      # プロジェクトローカルの pre-commit があれば優先実行
      GIT_DIR="$(git rev-parse --git-dir 2>/dev/null)" || exit 0
      LOCAL_HOOK="$GIT_DIR/hooks/pre-commit"
      if [ -x "$LOCAL_HOOK" ]; then
        exec "$LOCAL_HOOK" "$@"
      fi

      # pre-commit フレームワークの設定があれば使用
      if [ -f ".pre-commit-config.yaml" ] && command -v pre-commit >/dev/null 2>&1; then
        exec pre-commit run --hook-stage pre-commit "$@"
      fi

      # ========================================
      # デフォルト: ステージされたファイルをチェック
      # ========================================
      STAGED_FILES=$(git diff --cached --name-only --diff-filter=ACM)
      [ -z "$STAGED_FILES" ] && exit 0

      check_failed=0

      # Nix ファイルのチェック
      NIX_FILES=$(echo "$STAGED_FILES" | grep '\.nix$' || true)
      if [ -n "$NIX_FILES" ] && command -v nixpkgs-fmt >/dev/null 2>&1; then
        echo "🔍 Checking Nix format..."
        if ! echo "$NIX_FILES" | xargs nixpkgs-fmt --check 2>/dev/null; then
          echo "❌ Nix format check failed. Run: nixpkgs-fmt <files>"
          check_failed=1
        fi
      fi

      # Python ファイルのチェック
      PY_FILES=$(echo "$STAGED_FILES" | grep '\.py$' || true)
      if [ -n "$PY_FILES" ] && command -v ruff >/dev/null 2>&1; then
        echo "🔍 Checking Python format..."
        if ! ruff format --check $PY_FILES 2>/dev/null; then
          echo "❌ Python format check failed. Run: ruff format <files>"
          check_failed=1
        fi
        echo "🔍 Checking Python lint..."
        if ! ruff check $PY_FILES 2>/dev/null; then
          echo "❌ Python lint failed. Run: ruff check --fix <files>"
          check_failed=1
        fi
      fi

      # Markdown ファイルのチェック
      MD_FILES=$(git diff --cached --name-only --diff-filter=ACM -- '*.md' || true)
      if [ -n "$MD_FILES" ] && command -v markdownlint >/dev/null 2>&1; then
        echo "🔧 Auto-fixing Markdown lint..."
        git diff --cached --name-only --diff-filter=ACM -z -- '*.md' | xargs -0 markdownlint --fix -- 2>/dev/null || true
        git diff --cached --name-only --diff-filter=ACM -z -- '*.md' | xargs -0 git add --
        echo "🔍 Checking Markdown lint..."
        if ! git diff --cached --name-only --diff-filter=ACM -z -- '*.md' | xargs -0 markdownlint -- 2>/dev/null; then
          echo "❌ Markdown lint failed (unfixable issues remain)"
          check_failed=1
        fi
      fi

      # Rust ファイルのチェック
      RS_FILES=$(echo "$STAGED_FILES" | grep '\.rs$' || true)
      if [ -n "$RS_FILES" ]; then
        if command -v cargo >/dev/null 2>&1 && cargo locate-project &>/dev/null; then
          # Cargo プロジェクト内: cargo fmt を使用（edition は Cargo.toml から取得）
          echo "🔍 Checking Rust format..."
          if ! cargo fmt --check 2>/dev/null; then
            echo "❌ Rust format check failed. Run: cargo fmt"
            check_failed=1
          fi
        elif command -v rustfmt >/dev/null 2>&1; then
          # Cargo プロジェクト外: rustfmt を直接使用（edition 2024）
          echo "🔍 Checking Rust format..."
          if ! echo "$RS_FILES" | xargs rustfmt --edition 2024 --check 2>/dev/null; then
            echo "❌ Rust format check failed. Run: rustfmt --edition 2024 <files>"
            check_failed=1
          fi
        fi
      fi

      exit $check_failed
    '';
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
