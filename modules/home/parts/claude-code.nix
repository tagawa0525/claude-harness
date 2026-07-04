# =============================================================================
# Claude Code の設定
# =============================================================================
# Claude Code CLI、グローバル CLAUDE.md/hooks/skills/commands/scripts の同期、settings.json 管理、
# gh-pr-review 拡張のインストール
# （cc-bar 統合は ./modules/cc-bar.nix に集約）
# =============================================================================
{
  pkgs,
  lib,
  claudeCodeSource ? null,
  ...
}:

let
  # グローバルに登録する Claude Code hooks
  # nixos-rebuild 時に ~/.claude/settings.json へ自動登録される
  claudeGlobalHooks = [
    {
      file = "block-main-commit.sh";
      matcher = "Bash";
      timeout = 10000;
    }
    {
      file = "pre-merge-check.sh";
      matcher = "Bash";
      timeout = 30000;
    }
  ];

  # Claude Code settings.json の静的設定
  # nixos-rebuild 時に ~/.claude/settings.json へ自動反映される
  # hooks, statusLine, skipDangerousModePermissionPrompt は別途管理
  claudeCodeStaticSettings = {
    autoUpdatesChannel = "stable";
    enabledPlugins = {
      "code-simplifier@claude-plugins-official" = true;
      "rust-analyzer-lsp@claude-plugins-official" = true;
    };
    language = "Japanese";
    permissions = {
      allow = [
        # Skills
        "Skill(git-commit)"
        "Skill(git-branch)"
        "Skill(git-worktree)"
        "Skill(git-push)"
        "Skill(git-tidy)"
        "Skill(git-cherry-pick)"
        "Skill(git-info)"
        "Skill(gh-pr-merge)"
        "Skill(gh-pr-create)"
        "Skill(gh-pr-review)"
        "Skill(gh-actions-check)"
        # Git
        "Bash(git add:*)"
        "Bash(git branch:*)"
        "Bash(git checkout:*)"
        "Bash(git cherry-pick:*)"
        "Bash(git clean:*)"
        "Bash(git clone:*)"
        "Bash(git commit:*)"
        "Bash(git config:*)"
        "Bash(git diff:*)"
        "Bash(git diff-tree:*)"
        "Bash(git fetch:*)"
        "Bash(git log:*)"
        "Bash(git merge:*)"
        "Bash(git mv:*)"
        "Bash(git pull:*)"
        "Bash(git push:*)"
        "Bash(git rebase:*)"
        "Bash(git remote:*)"
        "Bash(git reset:*)"
        "Bash(git restore:*)"
        "Bash(git rev-list:*)"
        "Bash(git rm:*)"
        "Bash(git show:*)"
        "Bash(git stash:*)"
        "Bash(git status:*)"
        "Bash(git submodule:*)"
        "Bash(git switch:*)"
        "Bash(git symbolic-ref:*)"
        "Bash(git tag:*)"
        "Bash(git worktree:*)"
        # GitHub CLI
        "Bash(gh api:*)"
        "Bash(gh auth:*)"
        "Bash(gh pr:*)"
        "Bash(gh release:*)"
        "Bash(gh repo:*)"
        "Bash(gh run:*)"
        "Bash(gh workflow:*)"
        # Nix
        "Bash(nix search:*)"
        "Bash(nix build:*)"
        "Bash(nix eval:*)"
        "Bash(nix flake:*)"
        "Bash(nix-build:*)"
        "Bash(nix-instantiate:*)"
        "Bash(nix-prefetch-url:*)"
        "Bash(nixfmt:*)"
        "Bash(statix:*)"
        # 言語ツール
        "Bash(cargo:*)"
        "Bash(rustc:*)"
        "Bash(rustup:*)"
        "Bash(markdownlint:*)"
        "Bash(python3:*)"
        "Bash(node:*)"
        "Bash(npx:*)"
        # システム
        "Bash(systemctl:*)"
        "Bash(journalctl:*)"
        "Bash(mount:*)"
        "Bash(chmod:*)"
        "Bash(fish:*)"
        "Bash(direnv:*)"
        "Bash(podman:*)"
        "Bash(podman-compose:*)"
        # ユーティリティ
        "Bash(sort:*)"
        "Bash(echo:*)"
        "Bash(cat:*)"
        "Bash(file:*)"
        "Bash(test:*)"
        "Bash(grep:*)"
        "Bash(ls:*)"
        "Bash(tree:*)"
        "Bash(find:*)"
        "Bash(wc:*)"
        "Bash(jq:*)"
        "Bash(xargs:*)"
        "Bash(curl:*)"
        "Bash(hash:*)"
        "Bash(env)"
        # gh-pr-review スキルスクリプト
        "Bash(~/.claude/skills/gh-pr-review/scripts/get-pr-info.sh:*)"
        "Bash(~/.claude/skills/gh-pr-review/scripts/get-review-comments.sh:*)"
        "Bash(~/.claude/skills/gh-pr-review/scripts/reply-to-comment.sh:*)"
        # 共有スクリプト（PRレビュー待ち）
        "Bash(~/.claude/scripts/gh-wait-review.sh:*)"
        # Web
        "WebFetch(domain:api.github.com)"
        "WebFetch(domain:claude.ai)"
        "WebFetch(domain:code.visualstudio.com)"
        "WebFetch(domain:crates.io)"
        "WebFetch(domain:discourse.nixos.org)"
        "WebFetch(domain:docs.rs)"
        "WebFetch(domain:hub.docker.com)"
        "WebFetch(domain:gist.github.com)"
        "WebFetch(domain:github.com)"
        "WebFetch(domain:lib.rs)"
        "WebFetch(domain:mynixos.com)"
        "WebFetch(domain:nix-community.github.io)"
        "WebFetch(domain:raw.githubusercontent.com)"
        "WebFetch(domain:search.nixos.org)"
        "WebFetch(domain:www.anthropic.com)"
        "WebSearch"
      ];
      deny = [ "Bash(sudo:*)" ];
      defaultMode = "default";
    };
    plansDirectory = "docs/plans";
  };
in
{
  # ===========================================================================
  # Claude Code パッケージ
  # ===========================================================================
  home.packages = with pkgs; [
    llm-agents.claude-code # Claude Code CLI（自動更新）
    rsync # claude-sync スクリプトの実行時依存
  ];

  # .claude（CLAUDE.md/commands/skills/hooks/scripts）を ~/.claude に手動同期するコマンド
  # nixos-rebuild を待たずにスキル変更を反映する（bash/fish 共通で使用可）
  home.file.".local/bin/claude-sync" = {
    source = ../scripts/claude-sync.sh;
    executable = true;
  };

  # ===========================================================================
  # アクティベーションスクリプト
  # ===========================================================================

  # Claude Code グローバル設定の同期
  # flakeソースの .claude を ~/.claude に同期（ポリシーは scripts/claude-sync.sh を参照）
  # claudeCodeSourceがnullの場合は何もしない（オプトイン）
  home.activation.claudeCodeSetup = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    CLAUDE_DIR="$HOME/.claude"

    # .claude ディレクトリを作成
    mkdir -p "$CLAUDE_DIR"

    ${lib.optionalString (claudeCodeSource != null) ''
      # CLAUDE.md/commands/skills/hooks/scripts の同期（claude-sync コマンドと共通実装）
      # 同期ポリシーは modules/home/scripts/claude-sync.sh を参照
      PATH="${pkgs.rsync}/bin:$PATH" $DRY_RUN_CMD ${pkgs.bash}/bin/bash \
        ${../scripts/claude-sync.sh} "${claudeCodeSource}"
    ''}

    # 静的設定と hooks を settings.json に反映
    # claudeCodeSource の有無に関わらず常に実行（宣言的管理を保証）
    SETTINGS="$CLAUDE_DIR/settings.json"
    if [ ! -f "$SETTINGS" ]; then
      echo '{}' > "$SETTINGS"
    fi
    if [ "''${DRY_RUN:-0}" != "1" ]; then
      ${pkgs.jq}/bin/jq \
        --argjson managed '${builtins.toJSON claudeGlobalHooks}' \
        --argjson static '${builtins.toJSON claudeCodeStaticSettings}' \
        --arg hooks_dir "$CLAUDE_DIR/hooks" \
        '. + $static |
        .skipDangerousModePermissionPrompt = true |
        .hooks.PreToolUse |= (
          (. // []) as $existing |
          reduce ($managed | .[]) as $hook ($existing;
            ($hooks_dir + "/" + $hook.file) as $cmd |
            if any(.[]; any(.hooks[]?; .command | tostring | endswith($hook.file))) then
              map(
                if any(.hooks[]?; .command | tostring | endswith($hook.file)) then
                  .hooks |= map(
                    if .command | tostring | endswith($hook.file) then .command = $cmd else . end
                  )
                else . end
              )
            else
              . + [{"matcher": $hook.matcher, "hooks": [{"type": "command", "command": $cmd, "timeout": $hook.timeout}]}]
            end
          )
        )' \
        "$SETTINGS" > "$SETTINGS.tmp" && mv "$SETTINGS.tmp" "$SETTINGS"
      echo "Claude Code: settings and hooks updated in settings.json"
    else
      $DRY_RUN_CMD echo "Claude Code: (dry run) settings and hooks would be updated in settings.json"
    fi
  '';

  # GitHub CLI 拡張のインストール（gh-pr-review）
  # gh auth が完了している場合のみ実行
  home.activation.ghExtensions = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    # gh auth が完了しているか確認
    if ${pkgs.gh}/bin/gh auth status &>/dev/null; then
      # gh-pr-review がインストールされていない場合のみインストール
      if ! ${pkgs.gh}/bin/gh extension list 2>/dev/null | grep -q "agynio/gh-pr-review"; then
        $DRY_RUN_CMD ${pkgs.gh}/bin/gh extension install agynio/gh-pr-review
        $DRY_RUN_CMD echo "gh-pr-review extension installed"
      fi
    fi
  '';
}
