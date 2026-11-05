# =============================================================================
# Git関連の設定
# =============================================================================
# Git, Git Hooks, delta, GitHub CLI の設定
# =============================================================================
{ lib, pkgs, ... }:

let
  # 設計の記録（ADR）のテンプレートと検査（ADR-0009）。各プロジェクトの pre-commit が PATH から呼ぶ
  design-records = pkgs.callPackage ./design-records/package.nix { };
  # 文書の中のコミットの番号の付け直し（ADR-0012）。post-rewrite hook が store のパスで呼び、
  # 手で呼べるよう PATH にも入れる
  commit-refs = pkgs.callPackage ./commit-refs/package.nix { };
  # 履歴の中の崩れた語を辞書（./ja-terms/prh.yml）で直し、番号を commit-refs で付け直す（ADR-0045）。
  # 手で呼ぶツールなので PATH に入れる
  history-rewrite = pkgs.callPackage ./history-rewrite/package.nix { inherit commit-refs; };
  # 機密情報の検査（ADR-0015）。pre-commit と pre-merge-commit が store のパスで呼ぶ
  scanStagedSecrets = pkgs.writeShellApplication {
    name = "scan-staged-secrets";
    runtimeInputs = [
      pkgs.git
      pkgs.gitleaks
      pkgs.jq
    ];
    text = builtins.readFile ./scan-staged-secrets.sh;
  };
  scanSecrets = "${scanStagedSecrets}/bin/scan-staged-secrets";
  # プロジェクトローカルの hook のパス。ローカルの hook は linked worktree でも共通の .git/hooks にある
  # （--git-dir は worktree ごとの場所）。--git-path hooks/... は core.hooksPath（この hook 自身）を指すので
  # 使わない。検証: modules/home/parts/tests/local-hooks-worktree.sh（pre-commit、pre-merge-commit、
  # commit-msg）、modules/home/parts/tests/post-rewrite-remap.sh（post-rewrite）
  localHookPath = name: ''"$(git rev-parse --path-format=absolute --git-common-dir)/hooks/${name}"'';
  # 崩れた日本語の語の検査（ADR-0014）。pre-commit が ja-terms-staged を、commit-msg が commit-msg-check（中で
  # ja-terms-message）を store のパスで呼ぶ。textlint、設定、ja-terms-message、commit-msg-check は claude-hooks の
  # git merge と git commit のルールと共有する（./ja-terms/textlint.nix）
  jaTermsTextlint = pkgs.callPackage ./ja-terms/textlint.nix { };
  inherit (jaTermsTextlint) textlint;
  jaTermsCodeExtensions = jaTermsTextlint.codeExtensions;
  # 拡張子は大文字と小文字を問わない。prh の辞書（prh.yml）は誤った語を並べるので外す
  jaTermsPathspecs = map (ext: ":(icase,glob)**/*.${ext}") ([ "md" ] ++ jaTermsCodeExtensions) ++ [
    ":(exclude,glob)**/prh.yml"
  ];
  # 既定の検査（Nix、Python、Markdown、Rust）。ステージした版を見る（ADR-0028）。ツールは利用者の PATH のもの
  stagedChecks = pkgs.writeShellApplication {
    name = "staged-checks";
    runtimeInputs = [
      pkgs.git
      pkgs.jq
      pkgs.coreutils
      pkgs.diffutils
    ];
    text = builtins.readFile ./staged-checks.sh;
  };
  jaTermsStaged = pkgs.writeShellApplication {
    name = "ja-terms-staged";
    runtimeInputs = [
      pkgs.git
      pkgs.gawk
      textlint
    ];
    runtimeEnv = {
      JA_TERMS_CONFIG = "${jaTermsTextlint.config}";
      JA_TERMS_PATHSPECS = lib.concatStringsSep "\n" jaTermsPathspecs;
    };
    text = builtins.readFile ./ja-terms/staged.sh;
  };
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
  # プロジェクトローカルの .git/hooks/ があれば優先、なければプロジェクトの pre-commit の設定と
  # デフォルトチェック
  xdg.configFile."git/hooks/pre-commit" = {
    executable = true;
    text = ''
      #!/usr/bin/env bash
      set -euo pipefail

      # Claude Code セッションからの main/master 直接コミットをブロック
      # （PreToolUse hook と異なり必ず対象リポジトリ内で実行されるため、
      #   worktree や cd の失敗でも誤判定しない構造的なゲート。リモートの有無によらず止める
      #   （GitHub のリモートが無いリポジトリも、ブランチは /git-merge でローカルの main にマージする）。
      #   通すのは、flake.lock のみのコミット（nix-rebuild update の正規フロー）、マージを締めるコミット
      #   （MERGE_HEAD がある。/git-merge で本文の語に拒まれた後や競合の解決の後の git commit）、HEAD の無い
      #   リポジトリの最初のコミット。
      #   例外は modules/home/parts/claude-hooks/src/rules/block_main_commit.rs と必ず揃える
      #   → modules/home/parts/tests/main-commit-gates.sh が一致を検証する）
      if [ "''${CLAUDECODE:-}" = "1" ]; then
        BRANCH=$(git branch --show-current 2>/dev/null || echo "")
        if { [ "$BRANCH" = "main" ] || [ "$BRANCH" = "master" ]; } &&
          git rev-parse -q --verify HEAD >/dev/null &&
          ! git rev-parse -q --verify MERGE_HEAD >/dev/null; then
          # 変更(M)の flake.lock 1件のみ許可（削除・リネーム等は通さない）
          if [ "$(git diff --cached --name-status)" != "$(printf 'M\tflake.lock')" ]; then
            echo "❌ main / master ブランチへの直接コミットは禁止されています"
            echo "   featureブランチを作成してください: /git-branch"
            echo "   - GitHub のリモートが無いリポジトリでも、ブランチは /git-merge で main にマージするので、main に直接コミットする理由はありません（2026-11-06 に、cd が失敗した後の git commit が別のリポジトリの main に空のコミットを作りました）"
            echo "   - 試すための一時のリポジトリなら、main / master 以外のブランチで作る（git init -b <名前>、git switch -c <名前>）"
            echo "   - 通すのは、flake.lock だけの変更、マージを締めるコミット（MERGE_HEAD がある）、HEAD の無いリポジトリの最初のコミットです"
            exit 1
          fi
        fi
      fi

      # 機密情報の検査（ADR-0015）は ${scanSecrets} が行う。Claude Code のセッションのコミットで、コミットに
      # 入る中身（git add と commit -a の後の index）を見る。index を変える検査（Markdown の自動修正。#282。
      # ローカルの pre-commit）の後、ステージしたファイルが無くて抜ける前（種類の変更だけのコミット）、fork で
      # 抜ける前に呼ぶ
      # 検証: modules/home/parts/tests/pre-commit-secrets.sh

      # マージを締めるコミット（競合を解決した後の git commit / git merge --continue、
      # git merge --no-commit の後の git commit）では git が pre-merge-commit を呼ばないので、
      # 同じ検査をここで行う
      if git rev-parse -q --verify MERGE_HEAD >/dev/null; then
        "$(dirname "$0")/pre-merge-commit"
      fi

      # プロジェクトローカルの pre-commit があれば優先実行
      LOCAL_HOOK=${localHookPath "pre-commit"}
      if [ -x "$LOCAL_HOOK" ]; then
        # exec しないのは、ローカルの hook が index を変えた後に機密情報を検査するため
        "$LOCAL_HOOK" "$@"
        ${scanSecrets}
        exit 0
      fi

      check_failed=0

      # プロジェクトの pre-commit の設定（.pre-commit-config.yaml）があれば prek で実行する
      # （設計の記録の検査などへのオプトイン。ADR-0009）。core.hooksPath があると prek install は
      # 拒否されるので、ここから呼ぶ。exec しないのは、オプトインしたプロジェクトでも以下の既定の
      # 検査を実行するため。設定はそのプロジェクトの規約なので、fork でも実行する。
      # 検証: modules/home/parts/tests/pre-commit-prek.sh
      if [ -f ".pre-commit-config.yaml" ]; then
        if command -v prek >/dev/null 2>&1; then
          prek run --hook-stage pre-commit "$@" || check_failed=1
        else
          echo "⚠️  .pre-commit-config.yaml がありますが prek が PATH に無いので、その検査を飛ばします"
        fi
      fi

      # ========================================
      # デフォルト: ステージされたファイルをチェック
      # ========================================
      STAGED_FILES=$(git diff --cached --name-only --diff-filter=ACMRT)
      if [ -z "$STAGED_FILES" ]; then
        ${scanSecrets} || check_failed=1
        exit $check_failed
      fi

      # upstream リモートを持つ clone は他人のプロジェクトの fork（上流に PR を出す worktree）。
      # 以下の検査はどれもこちらのツールの版と規約（ruff の新しいルール、markdownlint の設定、
      # rustfmt の edition）を当てるもので、fork では上流の固定版と CI が正。新しい ruff は
      # 触っていない上流の行を上流の版にないルール（RUF043 等）で落とし、Markdown の自動修正は
      # 無関係な差分（``` → ```text 等）を上流向けのコミットに混ぜるので、fork では全部飛ばす。
      # 検証: modules/home/parts/tests/pre-commit-fork.sh
      if git remote get-url upstream >/dev/null 2>&1; then
        echo "⏭️  upstream リモートのある fork なので、こちらの規約の検査（Nix / Python / Markdown / Rust / 日本語の語）を飛ばします"
        ${scanSecrets} || check_failed=1
        exit $check_failed
      fi

      # Nix、Python、Markdown、Rust の書式と lint。ステージした版を見て、作業ツリーは書き換えない（ADR-0028）
      # 検証: modules/home/parts/tests/pre-commit-partial-stage.sh
      ${stagedChecks}/bin/staged-checks || check_failed=1

      # 日本語の崩れた語（ADR-0014）。Markdown の自動修正がステージし直した後の、コミットに入る中身
      # （ステージした版）を見る
      # 検証: modules/home/parts/tests/ja-terms.sh
      ${jaTermsStaged}/bin/ja-terms-staged || check_failed=1

      ${scanSecrets} || check_failed=1

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
  # 件名は 72 文字で失敗、50 文字超は警告（日本語件名の実態は 51〜72 が最多。判定は commit-msg-check）。
  # 手動コミットの自由度を残すため、pre-commit の main ガードと同じく CLAUDECODE=1 のときのみ
  xdg.configFile."git/hooks/commit-msg" = {
    executable = true;
    text = ''
      #!/usr/bin/env bash
      set -euo pipefail

      MSG_FILE="$1"

      # プロジェクトローカルの commit-msg があれば優先実行
      LOCAL_HOOK=${localHookPath "commit-msg"}
      if [ -x "$LOCAL_HOOK" ]; then
        exec "$LOCAL_HOOK" "$@"
      fi

      [ "''${CLAUDECODE:-}" = "1" ] || exit 0

      # upstream リモートを持つ clone は他人のプロジェクトの fork（上流に PR を出す worktree）。
      # Conventional Commits も件名の長さも日本語の語の辞書もこちらの規約で、上流の慣習（"Fix socket transport
      # when …" のような文。Serena の main には 72 文字を超える件名が普通にある）と衝突して
      # 上流向けのコミットを歪めるので、検査を全部飛ばす。pre-commit と同じ判定。
      # 検証: modules/home/parts/tests/commit-msg-conventions.sh
      if git remote get-url upstream >/dev/null 2>&1; then
        echo "⏭️  upstream リモートのある fork なので、こちらの規約の検査（Conventional Commits、件名の長さ、日本語の語）を飛ばします"
        exit 0
      fi

      # 件名の形式と長さ、日本語の語は commit-msg-check（./ja-terms/commit-msg-check.sh）が判定する。
      # claude-hooks の git commit の本文を先に検査するルールが同じものを呼ぶ（ADR-0073）。
      # 検証: modules/home/parts/tests/commit-msg-conventions.sh、modules/home/parts/tests/ja-terms.sh
      exec ${jaTermsTextlint.commitMsgCheck}/bin/commit-msg-check "$MSG_FILE"
    '';
  };

  # post-rewrite: rebase と amend で変わったコミットの番号を、作業ツリーの追跡している文書の中で
  # 付け直す（commit-refs remap）。検証の記録などに測ったコミットの番号を書く
  # リポジトリだけが要るので、`git config remap.commitRefs true` でオプトインする。置き換えは
  # コミットしない（どのコミットに含めるかは書き換えた人が決める）。
  # git は書き換えの後に呼ぶので、この hook の失敗は書き換えを止めない（表示するだけ）。
  # 検証: modules/home/parts/tests/post-rewrite-remap.sh
  xdg.configFile."git/hooks/post-rewrite" = {
    executable = true;
    text = ''
      #!/usr/bin/env bash
      set -euo pipefail

      # 旧と新の対応は標準入力で一度だけ渡されるので、ローカルの hook と置き換えの両方に渡すため取っておく
      INPUT=$(cat)

      # プロジェクトローカルの post-rewrite があれば先に呼ぶ。exec で置き換えないのは、ローカルの hook が
      # あるだけで置き換えが外れないようにするため。失敗しても置き換えは行い、終了コードは最後に返す
      LOCAL_HOOK=${localHookPath "post-rewrite"}
      local_rc=0
      if [ -x "$LOCAL_HOOK" ]; then
        printf '%s\n' "$INPUT" | "$LOCAL_HOOK" "$@" || local_rc=$?
      fi

      if [ "$(git config --type=bool --get remap.commitRefs || true)" != "true" ]; then
        exit "$local_rc"
      fi

      # rebase の途中（edit で止めた所）の amend でも git は呼ぶが、rebase の終わりにもう一度、
      # 元のコミットから最後のコミットへの対応を渡す。途中で置き換えると作業ツリーを汚して
      # rebase --continue に混ざり、途中の番号に置き換えた文書は最後の対応で引けなくなるので、
      # rebase の終わりに任せる
      if [ "$1" = "amend" ] && { [ -d "$(git rev-parse --git-path rebase-merge)" ] \
        || [ -d "$(git rev-parse --git-path rebase-apply)" ]; }; then
        exit "$local_rc"
      fi

      # 失敗したときに対応を残す場所。書き換えは済んでいて、この対応は二度と渡されないので、
      # 直してから手で呼び直せるようにする
      SAVED="$(git rev-parse --absolute-git-dir)/commit-refs-remap.input"
      echo "🔁 文書の中のコミットの番号を付け直します（remap.commitRefs）"
      if ! OUT=$(printf '%s\n' "$INPUT" | ${lib.getExe commit-refs} remap); then
        printf '%s\n' "$INPUT" > "$SAVED"
        echo "❌ コミットの番号を付け直せませんでした（書き換えたかどうかは、上の ERROR の最後の行にあります）"
        # 貼って使えるよう、パスをシェルの引数の形にする（空白などを含むリポジトリのため）
        echo "   修正方法: 上の ERROR を直してから、commit-refs remap $(printf '%q' "$SAVED")"
        exit 1
      fi
      printf '%s\n' "$OUT"
      if ! printf '%s\n' "$OUT" | grep -qx 'REFS: 0'; then
        echo "📝 置き換えはコミットしていません。git diff で確かめて、コミットしてください"
      fi
      exit "$local_rc"
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
