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
      #   例外は modules/home/parts/claude-hooks/src/rules/block_main_commit.rs と必ず揃える
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

      # マージを締めるコミット（競合を解決した後の git commit / git merge --continue、
      # git merge --no-commit の後の git commit）では git が pre-merge-commit を呼ばないので、
      # 同じ検査をここで行う
      if git rev-parse -q --verify MERGE_HEAD >/dev/null; then
        "$(dirname "$0")/pre-merge-commit"
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
      STAGED_FILES=$(git diff --cached --name-only --diff-filter=ACMR)
      [ -z "$STAGED_FILES" ] && exit 0

      # upstream リモートを持つ clone は他人のプロジェクトの fork（上流に PR を出す worktree）。
      # 以下の検査はどれもこちらの道具の版と規約（ruff の新しいルール、markdownlint の設定、
      # rustfmt の edition）を当てるもので、fork では上流の固定版と CI が正。新しい ruff は
      # 触っていない上流の行を上流の版にないルール（RUF043 等）で落とし、Markdown の自動修正は
      # 無関係な差分（``` → ```text 等）を上流向けのコミットに混ぜるので、fork では全部飛ばす。
      # 検証: modules/home/parts/tests/pre-commit-fork.sh
      if git remote get-url upstream >/dev/null 2>&1; then
        echo "⏭️  upstream リモートのある fork なので、こちらの規約の検査（Nix / Python / Markdown / Rust）を飛ばします"
        exit 0
      fi

      check_failed=0

      # Nix ファイルのチェック（NUL区切りでスペースを含むパスにも対応）
      NIX_FILES=$(git diff --cached --name-only --diff-filter=ACMR -- '*.nix' || true)
      if [ -n "$NIX_FILES" ] && command -v nixfmt >/dev/null 2>&1; then
        echo "🔍 Checking Nix format..."
        if ! git diff --cached --name-only --diff-filter=ACMR -z -- '*.nix' | xargs -0 nixfmt --check 2>/dev/null; then
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
      MD_FILES=$(git diff --cached --name-only --diff-filter=ACMR -- '*.md' || true)
      if [ -n "$MD_FILES" ] && command -v markdownlint >/dev/null 2>&1; then
        echo "🔧 Auto-fixing Markdown lint..."
        git diff --cached --name-only --diff-filter=ACMR -z -- '*.md' | xargs -0 markdownlint --fix -- 2>/dev/null || true
        # markdownlint --fix が直せない MD040（言語指定なし）/ MD060（CJK テーブル整列）を補完。
        # 実体は language-checks スキルの同期先（~/.claude）。無ければこの段は飛ばし、
        # 直後の markdownlint 検査で残った違反として検出される
        MD_FIXER="$HOME/.claude/skills/language-checks/scripts/fix-markdown-lint.py"
        if [ -f "$MD_FIXER" ] && command -v python3 >/dev/null 2>&1; then
          if ! git diff --cached --name-only --diff-filter=ACMR -z -- '*.md' | xargs -0 python3 "$MD_FIXER"; then
            echo "❌ fix-markdown-lint.py failed. Run: python3 $MD_FIXER <files>"
            check_failed=1
          fi
        fi
        git diff --cached --name-only --diff-filter=ACMR -z -- '*.md' | xargs -0 git add --
        echo "🔍 Checking Markdown lint..."
        if ! git diff --cached --name-only --diff-filter=ACMR -z -- '*.md' | xargs -0 markdownlint -- 2>/dev/null; then
          echo "❌ Markdown lint failed (unfixable issues remain)"
          check_failed=1
        fi
      fi

      # Rust ファイルのチェック
      # ステージ済み .rs ごとに最寄りの Cargo.toml を探し、クレート単位で cargo fmt --check する
      # （edition はそのクレートの Cargo.toml から取れる。hook はリポジトリ直下で実行されるため、
      #   直下に Cargo.toml が無いとサブディレクトリのクレートを見失っていた）。
      # クレート外の .rs は rustfmt を直接使う（edition 2024）。
      # 検証: modules/home/parts/tests/pre-commit-rust-format.sh
      RS_FILES=$(echo "$STAGED_FILES" | grep '\.rs$' || true)
      if [ -n "$RS_FILES" ]; then
        echo "🔍 Checking Rust format..."
        MANIFESTS=""
        LOOSE_RS=()
        while IFS= read -r f; do
          [ -n "$f" ] || continue
          d=$(dirname "$f")
          m=""
          while :; do
            if [ -f "$d/Cargo.toml" ]; then m="$d/Cargo.toml"; break; fi
            [ "$d" = "." ] && break
            d=$(dirname "$d")
          done
          if [ -n "$m" ]; then
            grep -qxF -- "$m" <<<"$MANIFESTS" || MANIFESTS="$MANIFESTS$m"$'\n'
          else
            LOOSE_RS+=("$f")
          fi
        done <<<"$RS_FILES"
        if [ -n "$MANIFESTS" ] && command -v cargo >/dev/null 2>&1; then
          while IFS= read -r m; do
            [ -n "$m" ] || continue
            if ! cargo fmt --check --manifest-path "$m" 2>/dev/null; then
              echo "❌ Rust format check failed. Run: cargo fmt --manifest-path $m"
              check_failed=1
            fi
          done <<<"$MANIFESTS"
        fi
        # パスに空白があっても壊れないよう NUL 区切りで渡す（Nix / Markdown の検査と同じ）
        if [ "''${#LOOSE_RS[@]}" -gt 0 ] && command -v rustfmt >/dev/null 2>&1; then
          if ! printf '%s\0' "''${LOOSE_RS[@]}" | xargs -0 rustfmt --edition 2024 --check 2>/dev/null; then
            echo "❌ Rust format check failed. Run: rustfmt --edition 2024 <files>"
            check_failed=1
          fi
        fi
      fi

      exit $check_failed
    '';
  };

  # pre-merge-commit: main / master へのマージコミットを、マージ結果が品質チェック
  # （run-checks.sh --merge。フォーマット・リント・テスト）を通ったときだけ作る。
  # git はマージを用意した後（作業ツリーと index がマージ結果の状態）、コミットの前に呼ぶ。
  # fast-forward はマージコミットを作らないので呼ばれない。競合を解決して締めるときは
  # 代わりに pre-commit が呼ばれるので、pre-commit が MERGE_HEAD を見てここに回す。
  #
  # 守るのはローカルのマージ（GitHub リモートのないリポジトリ。xlc など）。gh pr merge は
  # GitHub 側でマージするのでこの hook は走らない。GitHub のマージは CI と claude-hooks の
  # pre_merge_check（CI 成功、未解決スレッドなし、head が base より遅れていない）が守る
  # （必要なら GitHub の branch protection / merge queue も使える）。
  #
  # 対象は main / master だけ。統合先に入る内容を守るゲートで、ほかのブランチへのマージは
  # 途中経過（最終的に main へのマージで検査される）なので、テストを毎回走らせる重さに見合わない。
  # 手動コミットも対象（品質の基準は誰がマージしても同じ）。外すのは git merge --no-verify
  # （git の標準。pre-commit / commit-msg と同じ）。
  # 検証: modules/home/parts/tests/pre-merge-commit-checks.sh
  xdg.configFile."git/hooks/pre-merge-commit" = {
    executable = true;
    text = ''
      #!/usr/bin/env bash
      set -euo pipefail

      # プロジェクトローカルの pre-merge-commit があれば先に実行する（失敗すれば set -e で止まる）。
      # pre-commit / commit-msg と違って exec で置き換えない。置き換えると、ローカルの hook が
      # あるだけで --no-verify なしにこのゲートが外れる
      GIT_DIR="$(git rev-parse --git-dir 2>/dev/null)" || exit 0
      LOCAL_HOOK="$GIT_DIR/hooks/pre-merge-commit"
      if [ -x "$LOCAL_HOOK" ]; then
        "$LOCAL_HOOK" "$@"
      fi

      BRANCH=$(git branch --show-current 2>/dev/null || echo "")
      case "$BRANCH" in
        main|master) ;;
        *) exit 0 ;;
      esac

      # 他人のプロジェクトの fork では、こちらの道具の版と規約で検査しない（pre-commit と同じ判定）
      if git remote get-url upstream >/dev/null 2>&1; then
        echo "⏭️  upstream リモートのある fork なので、マージ結果の品質チェック（run-checks.sh）を飛ばします"
        exit 0
      fi

      # 検査は作業ツリーで走る。コミットされない変更が残っていると、マージコミットとは別の内容を
      # 検査してしまうので止める（比べる index は git が渡す GIT_INDEX_FILE。commit -a なら index.lock）
      DIRTY=$(git diff --name-only)
      if [ -n "$DIRTY" ]; then
        echo "❌ 作業ツリーにマージ結果以外の変更があるので、マージ結果を検査できません:"
        printf '%s\n' "$DIRTY" | sed 's/^/     /'
        echo "   直し方: git merge --abort で取り消し、変更をコミットするか git stash で退避してから、もう一度マージしてください"
        exit 1
      fi

      RUN_CHECKS="$HOME/.claude/skills/language-checks/scripts/run-checks.sh"
      if [ ! -f "$RUN_CHECKS" ]; then
        echo "❌ $RUN_CHECKS がないので、マージ結果を検査できません"
        echo "   直し方: claude-sync（または rebuild）で ~/.claude を同期してから、もう一度マージしてください"
        exit 1
      fi

      echo "🔍 マージ結果の品質チェック（run-checks.sh --merge）..."
      # git が hook に渡す GIT_INDEX_FILE（.git/index や index.lock）は、検査が起動するテストの中の
      # git にも引き継がれ、別のリポジトリでこの index を読み書きさせてしまうので外す
      # （--merge は HEAD と作業ツリーの差分で判定し、index に頼らない）
      if ! env -u GIT_INDEX_FILE bash "$RUN_CHECKS" --merge; then
        echo "❌ マージ結果が品質チェックに通らないので、マージコミットを作りません"
        echo "   直し方: git merge --abort で取り消し、マージするブランチで直してコミットしてから、もう一度マージしてください"
        exit 1
      fi
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

      # upstream リモートを持つ clone は他人のプロジェクトの fork（上流に PR を出す worktree）。
      # Conventional Commits も件名の長さもこちらの規約で、上流の慣習（"Fix socket transport
      # when …" のような文。Serena の main には 72 文字を超える件名が普通にある）と衝突して
      # 上流向けのコミットを歪めるので、検査を全部飛ばす。pre-commit と同じ判定。
      # 検証: modules/home/parts/tests/commit-msg-conventions.sh
      if git remote get-url upstream >/dev/null 2>&1; then
        echo "⏭️  upstream リモートのある fork なので、こちらの規約の検査（Conventional Commits、件名の長さ）を飛ばします"
        exit 0
      fi

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
