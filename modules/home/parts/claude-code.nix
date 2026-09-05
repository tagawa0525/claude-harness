# =============================================================================
# Claude Code の設定
# =============================================================================
# Claude Code CLI、グローバル CLAUDE.md/skills/commands/scripts の同期、PreToolUse hook
# （./claude-hooks、Rust 製 1 バイナリ）の配置、settings.json 管理
# （cc-bar 統合は ./modules/cc-bar.nix に集約）
# =============================================================================
{
  pkgs,
  lib,
  claudeCodeSource ? null,
  mattpocock-skills ? null,
  ...
}:

let
  # 外部リポジトリ由来のスキル。出所は flake input（rev は flake.lock）で記録し、
  # 更新は `nix flake update <input>` → rebuild で追従する。
  # 配備先ディレクトリは丸ごと上流のコピーとして扱う（ローカル編集は上書きされる）
  externalSkills = lib.optionals (mattpocock-skills != null) [
    {
      name = "grilling";
      src = "${mattpocock-skills}/skills/productivity/grilling";
    }
    {
      name = "grill-me";
      src = "${mattpocock-skills}/skills/productivity/grill-me";
    }
  ];

  # PreToolUse hook。10 本あった bash hook を Rust 製 1 バイナリ（./claude-hooks）に統合し、
  # settings.json には 1 件だけ登録する。Bash ツールの呼び出しごとに 1 プロセスで全ルールを評価する。
  # バイナリは store パスではなく固定パス ~/.claude/bin/claude-hooks 経由で参照する
  # （settings.json に store パスを書くと世代ごとに書き換わる。zellij.nix のプラグインと同じ理由）
  claude-hooks = pkgs.callPackage ./claude-hooks/package.nix { };
  claudeHooksBinRel = ".claude/bin/claude-hooks";
  claudeHookTimeout = 30000; # pre-merge-check が gh に数回問い合わせる
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
        "Bash(~/.claude/skills/gh-pr-review/scripts/get-latest-review.sh:*)"
        "Bash(~/.claude/skills/gh-pr-review/scripts/decide-next.sh:*)"
        "Bash(~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh:*)"
        "Bash(~/.claude/skills/gh-pr-review/scripts/request-rereview.sh:*)"
        "Bash(~/.claude/skills/gh-pr-review/scripts/reply-to-comment.sh:*)"
        # language-checks スキルスクリプト
        "Bash(~/.claude/skills/language-checks/scripts/run-checks.sh:*)"
        # 共有スクリプト（PRレビュー待ち、git/gh の決定的な手順）
        "Bash(~/.claude/scripts/gh-wait-review.sh:*)"
        "Bash(~/.claude/scripts/git-info.sh:*)"
        "Bash(~/.claude/scripts/worktree-add.sh:*)"
        "Bash(~/.claude/scripts/rename-branch.sh:*)"
        "Bash(~/.claude/scripts/rename-plan.sh:*)"
        "Bash(~/.claude/scripts/post-merge-cleanup.sh:*)"
        "Bash(~/.claude/scripts/gh-actions-diagnose.sh:*)"
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
    gitleaks # block-secret-commit hook が git commit 前に機密情報を検査する
  ];

  # PreToolUse hook バイナリ（settings.json はこの固定パスを参照する）
  home.file.${claudeHooksBinRel}.source = "${claude-hooks}/bin/claude-hooks";

  # .claude（CLAUDE.md/commands/skills/scripts）を ~/.claude に手動同期するコマンド
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
      # CLAUDE.md/commands/skills/scripts の同期（claude-sync コマンドと共通実装）
      # 同期ポリシーは modules/home/scripts/claude-sync.sh を参照
      PATH="${pkgs.rsync}/bin:$PATH" $DRY_RUN_CMD ${pkgs.bash}/bin/bash \
        ${../scripts/claude-sync.sh} "${claudeCodeSource}"
    ''}

    # 外部スキルの配備（externalSkills）。上流のコピーなので --delete で完全一致させる。
    # claude-sync コマンドは flake input を知らないため、ここ（rebuild）でのみ同期される
    ${lib.concatMapStringsSep "\n" (s: ''
      $DRY_RUN_CMD mkdir -p "$CLAUDE_DIR/skills/${s.name}"
      $DRY_RUN_CMD ${pkgs.rsync}/bin/rsync -a --delete --chmod=u+w "${s.src}/" "$CLAUDE_DIR/skills/${s.name}/"
    '') externalSkills}

    # 静的設定と hook を settings.json に反映
    # claudeCodeSource の有無に関わらず常に実行（宣言的管理を保証）
    SETTINGS="$CLAUDE_DIR/settings.json"
    if [ ! -f "$SETTINGS" ]; then
      echo '{}' > "$SETTINGS"
    fi
    if [ "''${DRY_RUN:-0}" != "1" ]; then
      ${pkgs.jq}/bin/jq \
        --arg cmd "$HOME/${claudeHooksBinRel} pre-tool-use" \
        --argjson timeout ${toString claudeHookTimeout} \
        --argjson legacy '${builtins.toJSON legacyHookFiles}' \
        --argjson static '${builtins.toJSON claudeCodeStaticSettings}' \
        '. + $static |
        .skipDangerousModePermissionPrompt = true |
        .hooks.PreToolUse |= (
          (. // [])
          # 旧 bash hook（ファイル名で識別）の登録を外し、空になった matcher を消す
          | map(.hooks |= map(select((.command | tostring) as $c | ($legacy | any(. as $f | $c | endswith("/" + $f))) | not)))
          | map(select((.hooks | length) > 0))
          # バイナリの登録を 1 件にする（あれば command / timeout を更新、なければ追加）
          | if any(.[]; any(.hooks[]?; (.command | tostring) | endswith("/bin/claude-hooks pre-tool-use"))) then
              map(.hooks |= map(if ((.command | tostring) | endswith("/bin/claude-hooks pre-tool-use")) then .command = $cmd | .timeout = $timeout else . end))
            else
              . + [{"matcher": "Bash", "hooks": [{"type": "command", "command": $cmd, "timeout": $timeout}]}]
            end
        )' \
        "$SETTINGS" > "$SETTINGS.tmp" && mv "$SETTINGS.tmp" "$SETTINGS"
      echo "Claude Code: settings and hooks updated in settings.json"
    else
      $DRY_RUN_CMD echo "Claude Code: (dry run) settings and hooks would be updated in settings.json"
    fi

    # 旧 bash hook の残骸を消す（claude-sync は --delete を使わないため残り続ける）。
    # activation は限られた PATH で実行されるため、外部コマンドはストアパスで参照する
    ${lib.concatMapStringsSep "\n" (
      f: ''$DRY_RUN_CMD ${pkgs.coreutils}/bin/rm -f "$CLAUDE_DIR/hooks/${f}"''
    ) legacyHookFiles}
    $DRY_RUN_CMD ${pkgs.coreutils}/bin/rm -f "$CLAUDE_DIR/hooks/lib/heredoc.sh"
    $DRY_RUN_CMD ${pkgs.coreutils}/bin/rmdir --ignore-fail-on-non-empty "$CLAUDE_DIR/hooks/lib" "$CLAUDE_DIR/hooks" 2>/dev/null || true
  '';
}
