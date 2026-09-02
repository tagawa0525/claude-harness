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
      # 初回 push で上流を自動設定する。スキルの手順から「上流の有無で -u を付け分ける」分岐をなくす
      push.autoSetupRemote = true;
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

      # Claude Code セッションからの main/master 直接コミットをブロック
      # （PreToolUse hook と異なり必ず対象リポジトリ内で実行されるため、
      #   worktree でも誤判定しない構造的なゲート。
      #   flake.lock のみのコミットは nix-rebuild update の正規フローなので除外。
      #   GitHub リモートがなければ PR を作れないので PR フロー適用外。
      #   例外は .claude/hooks/block-main-commit.sh と必ず揃える
      #   → modules/home/parts/tests/main-commit-gates.sh が一致を検証する）
      if [ "''${CLAUDECODE:-}" = "1" ] && git remote -v 2>/dev/null | grep -q 'github\.com'; then
        BRANCH=$(git branch --show-current 2>/dev/null || echo "")
        if [ "$BRANCH" = "main" ] || [ "$BRANCH" = "master" ]; then
          # 変更(M)の flake.lock 1件のみ許可（削除・リネーム等は通さない）
          if [ "$(git diff --cached --name-status)" != "$(printf 'M\tflake.lock')" ]; then
            echo "❌ mainブランチへの直接コミットは禁止されています"
            echo "   featureブランチを作成してください: /git-branch"
            exit 1
          fi
        fi
      fi

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

      # Nix ファイルのチェック（NUL区切りでスペースを含むパスにも対応）
      NIX_FILES=$(git diff --cached --name-only --diff-filter=ACM -- '*.nix' || true)
      if [ -n "$NIX_FILES" ] && command -v nixfmt >/dev/null 2>&1; then
        echo "🔍 Checking Nix format..."
        if ! git diff --cached --name-only --diff-filter=ACM -z -- '*.nix' | xargs -0 nixfmt --check 2>/dev/null; then
          echo "❌ Nix format check failed. Run: nixfmt <files>"
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
        # markdownlint --fix が直せない MD040（言語指定なし）/ MD060（CJK テーブル整列）を補完。
        # 実体は language-checks スキルの同期先（~/.claude）。無ければこの段は飛ばし、
        # 直後の markdownlint 検査で残った違反として検出される
        MD_FIXER="$HOME/.claude/skills/language-checks/scripts/fix-markdown-lint.py"
        if [ -f "$MD_FIXER" ] && command -v python3 >/dev/null 2>&1; then
          if ! git diff --cached --name-only --diff-filter=ACM -z -- '*.md' | xargs -0 python3 "$MD_FIXER"; then
            echo "❌ fix-markdown-lint.py failed. Run: python3 $MD_FIXER <files>"
            check_failed=1
          fi
        fi
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

  # commit-msg: Claude Code セッションのコミットに Conventional Commits を強制する。
  # 形式は決定的に判定できるため SKILL.md の文章ではなく hook で守る。
  # 件名は 72 文字で失敗、50 文字超は警告（日本語件名の実態は 51〜72 が最多）。
  # 手動コミットの自由度を残すため、pre-commit の main ガードと同じく CLAUDECODE=1 のときのみ
  xdg.configFile."git/hooks/commit-msg" = {
    executable = true;
    text = ''
      #!/usr/bin/env bash
      set -euo pipefail
      # 文字数を UTF-8 の文字単位で数える（LANG=C だとバイト数になり日本語件名が誤って超過する）
      export LC_ALL=C.UTF-8

      MSG_FILE="$1"

      # プロジェクトローカルの commit-msg があれば優先実行
      GIT_DIR="$(git rev-parse --git-dir 2>/dev/null)" || exit 0
      LOCAL_HOOK="$GIT_DIR/hooks/commit-msg"
      if [ -x "$LOCAL_HOOK" ]; then
        exec "$LOCAL_HOOK" "$@"
      fi

      [ "''${CLAUDECODE:-}" = "1" ] || exit 0

      # 最初の非コメント行（sed のみ。grep を挟むとコメント行だけのとき exit 1 で静かに落ちる）
      SUBJECT=$(sed -n '/^#/!{p;q}' "$MSG_FILE")

      # マージ・fixup/squash・Revert は Conventional Commits の対象外
      case "$SUBJECT" in
        Merge*|fixup!*|squash!*|Revert*) exit 0 ;;
      esac

      TYPES='feat|fix|docs|style|refactor|test|chore|perf|build|ci|revert'
      if ! printf '%s\n' "$SUBJECT" | grep -qE "^($TYPES)(\([^)]+\))?!?: [^ ]"; then
        echo "❌ Conventional Commits 形式ではありません: $SUBJECT"
        echo "   形式: <type>(<scope>)?: <subject>    type: $TYPES"
        exit 1
      fi

      LEN=''${#SUBJECT}
      if [ "$LEN" -gt 72 ]; then
        echo "❌ 件名が 72 文字を超えています ($LEN 文字): $SUBJECT"
        exit 1
      fi
      if [ "$LEN" -gt 50 ]; then
        echo "⚠️  件名が 50 文字を超えています ($LEN 文字)。短くできないか検討してください"
      fi
      exit 0
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
