# =============================================================================
# Git Hooks（グローバル）
# =============================================================================
# pre-commit、pre-merge-commit、commit-msg、post-rewrite と、hook が呼ぶ CLI。
# git の利用者の設定（programs.git の残り、delta、gh）は git.nix
# =============================================================================
{ pkgs, ... }:

let
  # 設計の記録（ADR）のテンプレートと検査（ADR-0009）。各プロジェクトの pre-commit が PATH から呼ぶ
  design-records = pkgs.callPackage ./design-records/package.nix { };
  # 文書の中のコミットの番号の付け直し（ADR-0012）。post-rewrite hook の中身は claude-hooks が包むものを呼ぶ。
  # 手で呼べるよう PATH に入れる
  commit-refs = pkgs.callPackage ./commit-refs/package.nix { };
  # 履歴の中の崩れた語を辞書（./ja-terms/prh.yml）で直し、番号を commit-refs で付け直す（ADR-0045）。
  # 手で呼ぶツールなので PATH に入れる
  history-rewrite = pkgs.callPackage ./history-rewrite/package.nix { inherit commit-refs; };
  # git hook の中身（ADR-0086）。PreToolUse hook と同じバイナリ（claude-code.nix）
  claude-hooks = pkgs.callPackage ./claude-hooks/package.nix { };
  # 既定の検査（Nix、Python、Markdown、Rust）。ステージした版を見る（ADR-0028）。ツールは利用者の PATH のもの。中身は
  # claude-hooks の src/staged_checks.rs（ADR-0086）。run-checks.sh の Markdown の段が PATH の staged-checks を呼ぶので、
  # それを呼ぶだけのコマンドを、run-checks.sh を移す #101 の段階 6 まで PATH に置く
  stagedChecks = pkgs.writeShellScriptBin "staged-checks" ''
    exec ${claude-hooks}/bin/claude-hooks git-hook staged-checks "$@"
  '';
in
{
  # prek: プロジェクトの .pre-commit-config.yaml を実行する（下の pre-commit hook が呼ぶ）
  # staged-checks: run-checks.sh の Markdown の段が呼ぶ既定の検査（ADR-0028。pre-commit は claude-hooks の中で同じ検査を呼ぶ）
  home.packages = [
    commit-refs
    design-records
    history-rewrite
    pkgs.prek
    stagedChecks
  ];

  # core.hooksPath は programs.git が有効なときだけ gitconfig に書かれる。利用者の設定（git.nix）に頼らず、ここでも有効にする
  programs.git.enable = true;
  programs.git.settings.core.hooksPath = "~/.config/git/hooks"; # グローバルhooksを使用

  # ===========================================================================
  # Git Hooks（グローバル）
  # ===========================================================================
  # pre-commit: Claude Code のセッションの main / master への直接のコミットを止め、マージを締めるコミットでは
  # pre-merge-commit を呼び、ローカルの .git/hooks/pre-commit があればそれと機密情報の検査だけ、無ければプロジェクトの
  # .pre-commit-config.yaml（prek）、既定の検査、崩れた日本語の語、機密情報の検査を行う。中身は claude-hooks の
  # src/git_hook.rs（ADR-0086）。main への直接のコミットの判定は、PreToolUse hook と同じ関数（main_commit_blocked）。
  # 検証: claude-hooks の tests/git_hook_pre_commit.rs（判定）、modules/home/parts/tests/ の pre-commit-*.sh、
  # main-commit-gates.sh、local-hooks-worktree.sh、ja-terms.sh（この hook のファイルと、包んだツール）
  xdg.configFile."git/hooks/pre-commit" = {
    executable = true;
    text = ''
      #!/bin/sh
      exec ${claude-hooks}/bin/claude-hooks git-hook pre-commit "$@"
    '';
  };

  # pre-merge-commit: main / master へのマージコミットを、マージ結果が品質チェック（run-checks.sh --merge。
  # フォーマット・リント・テスト）を通ったときだけ作る。fast-forward はマージコミットを作らないので呼ばれない。
  # 競合を解決して締めるときは代わりに pre-commit が呼ばれ、pre-commit が MERGE_HEAD を見てここを呼ぶ。
  #
  # 守るのはローカルのマージ（GitHub リモートのないリポジトリ。xlc など）。gh pr merge は GitHub 側でマージ
  # するのでこの hook は走らない。GitHub のマージは CI と claude-hooks の pre-merge-check が守る。対象は main /
  # master だけ（ほかのブランチへのマージは途中経過）。手動のマージも対象（品質の基準は誰がマージしても同じ）。
  # 外すのは git merge --no-verify（git の標準）。中身（ローカルの hook、機密情報の検査、作業ツリーの変更と
  # .gitignore から外れた生成物の判定、run-checks.sh の呼び出し）は claude-hooks の src/git_hook.rs（ADR-0086）。
  # 検証: claude-hooks の tests/git_hook_pre_merge_commit.rs（判定）、modules/home/parts/tests/pre-merge-commit-checks.sh、
  # local-hooks-worktree.sh、pre-commit-secrets.sh（この hook のファイルと、本物の run-checks.sh）
  xdg.configFile."git/hooks/pre-merge-commit" = {
    executable = true;
    text = ''
      #!/bin/sh
      exec ${claude-hooks}/bin/claude-hooks git-hook pre-merge-commit "$@"
    '';
  };

  # commit-msg: Claude Code セッションのコミットに Conventional Commits を強制する。
  # 形式は決定的に判定できるため SKILL.md の文章ではなく hook で守る。
  # 件名は 72 文字で失敗、50 文字超は警告（日本語件名の実態は 51〜72 が最多。判定は `ja-terms commit-msg`）。
  # 中身（ローカルの hook、Claude Code の外と fork の対象外、ja-terms への受け渡し）は claude-hooks の
  # src/git_hook.rs（ADR-0086）。手で打つコミットの自由を残すため、CLAUDECODE=1 のときだけ検査する。
  # 検証: claude-hooks の tests/git_hook_commit_msg.rs（判定）、modules/home/parts/tests/commit-msg-conventions.sh と
  # local-hooks-worktree.sh（この hook のファイルと、包んだ ja-terms）
  xdg.configFile."git/hooks/commit-msg" = {
    executable = true;
    text = ''
      #!/bin/sh
      exec ${claude-hooks}/bin/claude-hooks git-hook commit-msg "$@"
    '';
  };

  # post-rewrite: rebase と amend で変わったコミットの番号を、作業ツリーの追跡している文書の中で
  # 付け直す（commit-refs remap）。`git config remap.commitRefs true` でオプトインする。中身（ローカルの hook、
  # rebase の途中の amend、失敗したときに対応を残すこと）は claude-hooks の src/git_hook.rs（ADR-0086）。
  # commit-refs は claude-hooks の package.nix が包むものを使う。
  # 検証: claude-hooks の tests/git_hook_post_rewrite.rs（判定）、modules/home/parts/tests/post-rewrite-remap.sh
  # （この hook のファイルと、包んだ commit-refs）
  xdg.configFile."git/hooks/post-rewrite" = {
    executable = true;
    text = ''
      #!/bin/sh
      exec ${claude-hooks}/bin/claude-hooks git-hook post-rewrite "$@"
    '';
  };
}
