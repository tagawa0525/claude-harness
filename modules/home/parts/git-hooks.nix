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
  # プロジェクトローカルの hook のパス。ローカルの hook は linked worktree でも共通の .git/hooks にある
  # （--git-dir は worktree ごとの場所）。--git-path hooks/... は core.hooksPath（この hook 自身）を指すので
  # 使わない（commit-msg の中身は claude-hooks の src/git_hook.rs が同じ場所を探す）。検証:
  # modules/home/parts/tests/local-hooks-worktree.sh（pre-commit、pre-merge-commit、commit-msg）、
  # modules/home/parts/tests/post-rewrite-remap.sh（post-rewrite）
  localHookPath = name: ''"$(git rev-parse --path-format=absolute --git-common-dir)/hooks/${name}"'';
  # git hook の中身（ADR-0086）。PreToolUse hook と同じバイナリ（claude-code.nix）
  claude-hooks = pkgs.callPackage ./claude-hooks/package.nix { };
  # 機密情報の検査（ADR-0015。claude-hooks の src/git_hook.rs の scan-secrets）。pre-commit と pre-merge-commit が呼び、
  # gitleaks は claude-hooks の package.nix が包むものを使う
  scanSecrets = "${claude-hooks}/bin/claude-hooks git-hook scan-secrets";
  # 既定の検査（Nix、Python、Markdown、Rust）。ステージした版を見る（ADR-0028）。ツールは利用者の PATH のもの。中身は
  # claude-hooks の src/staged_checks.rs（ADR-0086）。run-checks.sh の Markdown の段が PATH の staged-checks を呼ぶので、
  # それを呼ぶだけのコマンドを、run-checks.sh を移す #101 の段階 6 まで PATH に置く
  stagedChecks = pkgs.writeShellScriptBin "staged-checks" ''
    exec ${claude-hooks}/bin/claude-hooks git-hook staged-checks "$@"
  '';
in
{
  # prek: プロジェクトの .pre-commit-config.yaml を実行する（下の pre-commit hook が呼ぶ）
  # staged-checks: pre-commit の既定の検査。run-checks.sh の Markdown の段も呼ぶ（ADR-0028）
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

  # pre-merge-commit: main / master へのマージコミットを、マージ結果が品質チェック
  # （run-checks.sh --merge。フォーマット・リント・テスト）を通ったときだけ作る。
  # git はマージを用意した後（作業ツリーと index がマージ結果の状態）、コミットの前に呼ぶ。
  # fast-forward はマージコミットを作らないので呼ばれない。競合を解決して締めるときは
  # 代わりに pre-commit が呼ばれるので、pre-commit が MERGE_HEAD を見てここに回す。
  #
  # 守るのはローカルのマージ（GitHub リモートのないリポジトリ。xlc など）。gh pr merge は
  # GitHub 側でマージするのでこの hook は走らない。GitHub のマージは CI と claude-hooks の
  # pre-merge-check（CI 成功、未解決スレッドなし、head が base より遅れていない）が守る
  # （必要なら GitHub の branch protection / merge queue も使える）。
  #
  # 対象は main / master だけ。統合先に入る内容を守るゲートで、ほかのブランチへのマージは
  # 途中経過（最終的に main へのマージで検査される）なので、テストを毎回実行する重さに見合わない。
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
      LOCAL_HOOK=${localHookPath "pre-merge-commit"}
      if [ -x "$LOCAL_HOOK" ]; then
        "$LOCAL_HOOK" "$@"
      fi

      # 機密情報の検査（ADR-0015）。競合の無いマージでは pre-commit が走らないので、相手のブランチから入る
      # 中身をここで見る（main 以外へのマージも）
      # 検証: modules/home/parts/tests/pre-commit-secrets.sh
      ${scanSecrets}

      BRANCH=$(git branch --show-current 2>/dev/null || echo "")
      case "$BRANCH" in
        main|master) ;;
        *) exit 0 ;;
      esac

      # 他人のプロジェクトの fork では、こちらのツールの版と規約で検査しない（pre-commit と同じ判定）
      if git remote get-url upstream >/dev/null 2>&1; then
        echo "⏭️  upstream リモートのある fork なので、マージ結果の品質チェック（run-checks.sh）を飛ばします"
        exit 0
      fi

      # 検査は作業ツリーで走る。コミットされない変更が残っていると、マージコミットとは別の内容を
      # 検査してしまうので止める（比べる index は git が渡す GIT_INDEX_FILE。commit -a なら index.lock）。
      # 追跡していないファイルも、ruff check . や pytest のような木全体の検査が拾うので含める。
      # .gitignore で除外したもの（ビルド成果物など）は検査のツールも除外するので含めない
      # core.quotePath=false: 下の .gitignore から外れた生成物の一覧と同じく、非 ASCII のパスを 8 進で
      # 引用せずに出す（書き方が違うと、同じファイルが別のものに見える）
      MODIFIED=$(git -c core.quotePath=false diff --name-only)
      UNTRACKED=$(git -c core.quotePath=false ls-files --others --exclude-standard)
      if [ -n "$MODIFIED$UNTRACKED" ]; then
        # マージするブランチが .gitignore から行を外すと、HEAD の作業ツリーに残っていた生成物が
        # 未追跡として現れる。コミットするものではないので、HEAD の .gitignore で無視されていた
        # ファイルを見分け、別の一覧で示す。git check-ignore は作業ツリー（マージ結果）の .gitignore しか
        # 読まないので、HEAD の .gitignore だけを置いた一時リポジトリで判定する（ファイルの実体は
        # 要らない。パスとパターンで決まる）。git archive は export-ignore の属性で .gitignore を
        # 落としうるので、git show で 1 本ずつ取り出す。
        # 判定の失敗で hook ごと終わると、変更の一覧と修正方法が出ないので、関数の中で失敗を return 1 に
        # 変えて受ける。if の条件の中では set -e が効かないので、各段に || return 1 を書く。
        # 関数は $( ) の subshell で走るので、EXIT の trap はその subshell の終わりに一時ディレクトリを消す
        # （dir を local にしないのは、trap が走る時点で関数を抜けていて local が消えているため）
        head_ignored() {
          local f rc=0
          dir=$(mktemp -d) || return 1
          trap 'rm -rf "$dir"' EXIT
          mkdir "$dir/repo" || return 1
          git ls-tree -r -z --name-only HEAD > "$dir/tree" || return 1
          while IFS= read -r -d "" f; do
            case "$f" in .gitignore|*/.gitignore) ;; *) continue ;; esac
            mkdir -p "$dir/repo/$(dirname "$f")" || return 1
            git show "HEAD:$f" > "$dir/repo/$f" || return 1
          done < "$dir/tree"
          # hook に渡る GIT_INDEX_FILE などが一時リポジトリの git にも効かないように外す
          env -u GIT_INDEX_FILE -u GIT_DIR -u GIT_WORK_TREE git -C "$dir/repo" init -q || return 1
          # 上の UNTRACKED をそのまま渡す（未追跡の走査を 2 度しない）。-z でなく行で渡すのは、
          # ls-files と check-ignore が同じ C の引用（タブ、改行、"、\）で書き、読み戻すため。
          # check-ignore の終了値 1 は「どれも無視されない」
          printf '%s\n' "$UNTRACKED" \
            | env -u GIT_INDEX_FILE -u GIT_DIR -u GIT_WORK_TREE \
                git -C "$dir/repo" -c core.quotePath=false check-ignore --stdin || rc=$?
          [ "$rc" -le 1 ]
        }
        # --no-renames: ディレクトリを移したブランチで、改名として新しい .gitignore だけを出すと、
        # 無視の行を持っていた元の .gitignore が落ちる
        IGNORE_CHANGED=$(git -c core.quotePath=false diff --cached --no-renames --name-only HEAD -- ':(glob)**/.gitignore')
        UNIGNORED=""
        MARK_FAILED=""
        if [ -n "$IGNORE_CHANGED" ] && [ -n "$UNTRACKED" ]; then
          if ! UNIGNORED=$(head_ignored); then
            UNIGNORED=""
            MARK_FAILED=1
          fi
        fi
        # 印の付いたファイルは下の一覧にだけ出す（同じファイルを 2 度並べると、大きな出力のディレクトリで
        # 修正方法が埋もれる）
        if [ -n "$UNIGNORED" ]; then
          OTHERS=$(printf '%s\n' "$MODIFIED" "$UNTRACKED" | grep -v -x -F -e "" -f <(printf '%s\n' "$UNIGNORED") || true)
        else
          OTHERS=$(printf '%s\n' "$MODIFIED" "$UNTRACKED" | grep -v -x -e "" || true)
        fi
        echo "❌ 作業ツリーにマージ結果以外の変更があるので、マージ結果を検査できません:"
        if [ -n "$OTHERS" ]; then
          printf '%s\n' "$OTHERS" | sed 's/^/     /'
        fi
        if [ -n "$UNIGNORED" ]; then
          echo "   次のファイルは、マージするブランチが .gitignore から外した生成物（HEAD の .gitignore では無視されていた。ブランチが生成する処理を消したか置き換えたなど）:"
          printf '%s\n' "$UNIGNORED" | sed 's/^/     /'
          echo "   マージで変わる .gitignore:"
          printf '%s\n' "$IGNORE_CHANGED" | sed 's/^/     /'
          echo "   修正方法: git merge --abort で取り消す。上の生成物は、DVC のキャッシュなどに記録された版と中身が同じで戻せるなら作業ツリーから消す。記録された版と中身が違うか、記録が無ければ、消すと戻せないので、何の出力でなぜ違うかの原因を調べてから消すか移すかを決める（残して無視し続けるなら、マージするブランチの .gitignore に行を戻す）。ほかの変更はコミットして（不要なファイルは消して）から、もう一度マージしてください"
        else
          if [ -n "$MARK_FAILED" ]; then
            echo "   マージで次の .gitignore が変わるが、HEAD の .gitignore で無視されていたファイルの判定に失敗した（理由は上のエラー）。上の一覧には、マージするブランチが .gitignore から外した生成物が混ざりうる:"
            printf '%s\n' "$IGNORE_CHANGED" | sed 's/^/     /'
          fi
          echo "   修正方法: git merge --abort で取り消し、変更をコミットして（不要なファイルは消して）から、もう一度マージしてください"
        fi
        exit 1
      fi

      RUN_CHECKS="$HOME/.claude/skills/language-checks/scripts/run-checks.sh"
      if [ ! -f "$RUN_CHECKS" ]; then
        echo "❌ $RUN_CHECKS がないので、マージ結果を検査できません"
        echo "   修正方法: claude-sync（または rebuild）で ~/.claude を同期してから、もう一度マージしてください"
        exit 1
      fi

      echo "🔍 マージ結果の品質チェック（run-checks.sh --merge）..."
      # git が hook に渡す GIT_INDEX_FILE（.git/index や index.lock）は、検査が起動するテストの中の
      # git にも引き継がれ、別のリポジトリでこの index を読み書きさせてしまうので外す
      # （--merge は HEAD と作業ツリーの差分で判定し、index に頼らない）
      if ! env -u GIT_INDEX_FILE bash "$RUN_CHECKS" --merge; then
        echo "❌ マージ結果が品質チェックに通らないので、マージコミットを作りません"
        echo "   修正方法: git merge --abort で取り消し、マージするブランチで直してコミットしてから、もう一度マージしてください"
        exit 1
      fi
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
