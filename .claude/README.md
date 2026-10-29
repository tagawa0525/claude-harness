# .claude の構成

Claude Code のグローバル設定。`claude-sync`（または home-manager の activation）で
`~/.claude` に同期される（同期ポリシーは `modules/home/scripts/claude-sync.sh`）。

## 役割の分担

SKILL.md（文章）・script（手順）・hook（ゲート）の使い分け:

| 置き場所 | 何を置くか                                                                 | 例                                                |
| -------- | -------------------------------------------------------------------------- | ------------------------------------------------- |
| hook     | コマンド文字列や git/GitHub の状態から機械的に判定できる「禁止・必須」     | main への commit/push、PR 本文の見出し            |
| script   | 入力が決まれば結果が一意に決まる手順（数える・比べる・探す・決まった順序） | worktree の命名と作成、マージ後の片付け、状態表示 |
| SKILL.md | 判断が必要なこと                                                           | 命名、要約、指摘の分類、見送りの判断              |

文章は読み飛ばされうるし、スキルを経由しない操作には効かない。決定的に判定できるルールは hook に、
判断の要らない手順は script に置き、SKILL.md には判断が必要な部分だけを残す。

## スクリプトの置き場所

| 場所                                  | 用途                                                                                                                                                                                    | 例                                                                                                       |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `../modules/home/parts/claude-hooks/` | Claude Code の PreToolUse hook（Rust 製 1 バイナリ）。`claude-code.nix` がビルドして `~/.claude/bin/claude-hooks` に置き、settings.json に 1 件だけ登録する（matcher は Bash と Agent） | `src/rules/block_main_commit.rs`, `src/shell.rs`                                                         |
| `scripts/`                            | 複数スキルから呼ばれる、または git/gh に対する汎用の手順                                                                                                                                | `gh-wait-review.sh`, `git-info.sh`, `worktree-add.sh`, `post-merge-cleanup.sh`, `gh-actions-diagnose.sh` |
| `skills/<name>/scripts/`              | そのスキル専用のスクリプト                                                                                                                                                              | `gh-pr-review/scripts/decide-next.sh`                                                                    |
| `skills/language-checks/scripts/`     | 言語別の品質チェック・自動修正ツール。language-checks を参照する全スキル（git-commit / gh-pr-review 等）から使う                                                                        | `run-checks.sh`, `fix-markdown-lint.py`                                                                  |
| `tests/`                              | hook のルール / scripts / skills のテスト（`claude-sync` の同期対象外）。scripts のテストは対象ごとに `tests/scripts/` に分け、`scripts.sh` がまとめて実行する                          | `hooks.sh`, `hooks-diff.sh`, `scripts.sh`, `scripts/gh-pr-review.sh`, `skills.sh`                        |

判断基準:

- 1 つのスキルだけが使う → `skills/<name>/scripts/`
- 言語やファイル種別に対するチェック・修正 → `skills/language-checks/scripts/`
  （呼び出し元がスキルであってもここに置く。使う側ではなく対象で分類する）
- 上記以外で複数スキルが使う、または git/gh に対する汎用の手順 → `scripts/`
- Claude Code が settings.json 経由で起動するもの → `claude-hooks` のルール（`src/rules/` に 1 ファイル）

## 外部リポジトリ由来のスキル

`~/.claude/skills/` には、このリポジトリの `.claude/skills/` 以外に外部リポジトリの
スキルもデプロイされる。出所は flake input として `flake.nix` / `flake.lock` に記録し、
対象は `modules/home/parts/claude-code.nix` の `externalSkills` で列挙する。

| スキル                 | 出所                                                                             |
| ---------------------- | -------------------------------------------------------------------------------- |
| `grilling`, `grill-me` | [mattpocock/skills](https://github.com/mattpocock/skills) `skills/productivity/` |

- 更新追従: `nix flake update mattpocock-skills` → rebuild
- デプロイ先は上流の完全なコピー（`rsync --delete`）。ローカルで編集しても rebuild で戻る
- `claude-sync` コマンドは flake input を知らないため、外部スキルは rebuild でのみ同期される

## 文脈を足す hook

禁止ではなく文脈を足す hook は、`claude-code.nix` が settings.json に登録する（`claude-hooks` のルールではない）。

- `UserPromptSubmit`: ターンごとに `date '+現在時刻: %F %H:%M %Z'` の 1 行を文脈に入れる。報告とメモリに書く時刻を推測しないため（推定で数時間先の時刻を書いた）

## hook が強制するルール

決定的に判定できるルールは SKILL.md の文章ではなく hook で強制する（迂回できないゲート）。

| ルール                      | 強制するルール                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | エスケープ                                                                                                                                                         |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `block-main-commit`         | main / master での `git commit`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | なし                                                                                                                                                               |
| `block-secret-commit`       | グローバルの git の hook を使わない（ローカルの core.hooksPath を持つ）リポジトリの、コミット対象に機密情報がある `git commit`。gitleaks（`git --staged`、`-a` なら `--pre-commit` も）。ほかのリポジトリは git の pre-commit が検査する（ADR-0015）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | 行に `gitleaks:allow`（`.gitleaksignore` / `.gitleaks.toml` も可）、または `ALLOW_SECRET_COMMIT=<理由>`                                                            |
| `block-no-verify`           | git の hook の検査を飛ばすもの: `git commit` / `merge` / `pull` の `--no-verify`（省略形、commit の `-n`）、core.hooksPath の変更（`-c` / `--config-env` / `git config`、`GIT_CONFIG_*`）、`CLAUDECODE` の書き換え（ADR-0015）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `ALLOW_NO_VERIFY=<理由>`                                                                                                                                           |
| `guard-branch-base`         | 既定ブランチ以外の上で、起点を省いたブランチ作成（`git switch -c` / `checkout -b` / `branch <b>` / `worktree add -b`、パスの名前で作る `worktree add <path>`）。作業中のブランチの未完了のコミットが別のトピックに入るのを防ぐ                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | なし（続きにするなら起点を明示する。HEAD でもよい）                                                                                                                |
| `guard-git-add`             | 対象を絞らない `git add`（`-A` / `--all` / `.` / `:/` / `*`）。パスで範囲を限定した `-A src/` と `-u` は可                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | コマンドに `ALLOW_GIT_ADD_ALL=<理由>` を付ける                                                                                                                     |
| `guard-git-push`            | main / master への push、force push（`--force-with-lease` は feature branch なら open PR があっても可）、`--all` / `--mirror`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | コマンドに `ALLOW_PROTECTED_PUSH=<理由>` を付ける                                                                                                                  |
| `guard-gh-api`              | 生の `gh api` による Actions 権限設定（`actions/permissions`、`default_workflow_permissions`）への書き込みと、GraphQL の `resolveReviewThread`（resolve は `resolve-thread.sh` 経由に固定）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | なし                                                                                                                                                               |
| `guard-gh-run-rerun`        | 既に attempt 2 以上の run への `gh run rerun`（一時障害の再実行は 1 回まで。確認できなければ deny）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | コマンドに `ALLOW_RERUN=<理由>` を付ける                                                                                                                           |
| `check-merge-body-ja-terms` | `git merge` の `-m` / `-F` の本文の崩れた日本語の語（commit-msg と同じ ja-terms-message）。git は数分の pre-merge-commit の後に commit-msg を呼ぶので、先に止める。commit-msg が呼ばれない形（`--ff-only`、`--squash`、`--no-commit`、`--no-verify`、グローバルの core.hooksPath を使わないリポジトリ）と、シェルが展開する本文（判定できない理由を出す）と、commit-msg が語を検査しないリポジトリは対象外（ADR-0050）                                                                                                                                                                                                                                                                                                                                                                                            | なし（語を置き換える。同じコマンドの前段で -F のファイルを直す形は、呼び出しを分けるよう理由に示す）                                                               |
| `guard-git-merge`           | 既定ブランチへのローカルの `git merge`（GitHub リモートのないリポジトリの /git-merge）に `--no-ff` を付けること、対象のブランチが既定ブランチの上に rebase 済みであること（`gh pr merge` の `--merge` と「head が base より遅れていない」に相当）。対象のブランチの worktree に未コミットの変更と未追跡のファイル（無視されたものを除く）が無いこと（後片付けの `git worktree remove` が止まる）。対象の判定は `pre-git-merge-check` と共有する                                                                                                                                                                                                                                                                                                                                                                   | なし（修正方法は理由に示す）                                                                                                                                       |
| `guard-git-stash`           | `git stash`（`list` と `show` を除く。`-C` つきや `push` / `pop` / `drop` なども）。stash は全 worktree と全セッションで共有なので使わず、WIP のコミットか対象のファイルだけのステージで退避する                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | なし（修正方法は理由に示す）                                                                                                                                       |
| `guard-git-reset-hard`      | 追跡しているファイルに未コミットの変更がある worktree での `git reset --hard`。前に `cd` があり、絶対パスの `-C` / `env -C` で指していなければ、hook の cwd（絶対パスの `env -C` で包んだ `bash -c` の中ならその先）と前の各 `cd` の直前のディレクトリ（`cd` が失敗すると reset はそこで走る）も見る。前に `cd` が無ければ、`-C` / `env -C` で決まるディレクトリだけを見る。変数のパスや `GIT_DIR` などディレクトリを決められない形も止める。worktree はほかのセッションと共有しうるので、対象は `git -C <絶対パス>` で指し、残す変更は WIP のコミットにする                                                                                                                                                                                                                                                      | `ALLOW_GIT_RESET_HARD=<理由>`                                                                                                                                      |
| `guard-git-amend`           | 名前のある、既定ブランチでないブランチで、先端が既定ブランチに含まれる（`git merge-base --is-ancestor`）ときの `git commit --amend`（`--am` などの略、`-C` / `cd` の先も。同じコマンドの前の `git switch` はその先のブランチで見る）。最初の `git commit` が hook に拒まれたのに気づかず amend すると、既定ブランチのコミット（マージコミットなら親 2 つ）を書き換えたものがブランチに入る。既定ブランチの上、detached HEAD、ブランチに固有のコミットがあるときの amend は通す                                                                                                                                                                                                                                                                                                                                    | なし（修正方法は理由に示す）                                                                                                                                       |
| `guard-exclusive-slurm`     | 排他の Slurm のジョブ（squeue で OverSubscribe が NO）が実行中の、全コアを使う手元のコマンド（`cargo` の build / check / test / clippy / run / bench（alias も）、`dvc repro`、`nix build`、`nix flake check`、`bin/` と `release`・`debug` ディレクトリの xs / lattice / core / chain。`nix develop`・`nix shell` の `-c` の中も）と、何を実行するか決められないコマンド。時間の記録を乱す。1 コアの `taskset` なら可（`-j 1` は限りにならない）。`run-checks.sh` と `run-python-tests.sh` は対象にしない（`run-checks.sh` が自分で 1 コアに限る）。squeue が無いときは通し、失敗したときは通して理由を出す（ADR-0041）                                                                                                                                                                                          | なし（修正方法は理由に示す）                                                                                                                                       |
| `pre-merge-check`           | `gh pr merge` の `--merge` / `--delete-branch` / 本文見出し、CI、reviewDecision、未解決スレッド、head が base より遅れていない（リベース済み）、最後の push が自動レビュー済み（bot レビューの commit_id が head と一致。リベースだけで hash が変わり、base からの PR の差分がレビュー済みと同じなら一致とみなす（ADR-0021）。bot レビューが無いリポジトリでは検査しない。変更ファイルがすべて `docs/` 配下で一度レビューが成功していれば、最後の push が未レビューでも通す（ADR-0004）。head にローカルレビューの記録（High なし）があれば、Copilot のレビューの有無・到着・内容を問わず通す（ADR-0043。Copilot の指摘は未解決スレッドの検査が見る）。最後の記録（High なし）の SHA から rebase だけした head も同じ（ADR-0059））。レビュー自体が失敗しているときは CI 失敗に数えずレビュー未実施として報告する | コマンドに `ALLOW_UNREVIEWED_HEAD=<理由>` を付ける                                                                                                                 |
| `pre-git-merge-check`       | 既定ブランチへのローカルのブランチの `git merge` と、`gh pr merge`（`pre-merge-check` の中で同じ判定）と `gh pr create`（`pre-pr-create-check` の中で同じ判定。fork、`owner:branch` の `--head`、`-R` / `--repo` / `GH_REPO` で origin と別のリポジトリ、移れない cd 先（それ自体は別の理由で止める）では数えず、base や head が手元に無く数えられなければ通す。ADR-0056）で、ブランチが新しい ADR（`docs/adr/` 直下の `NNNN-*.md`）を 2 件以上加えていれば止める（1 ブランチ 1 トピック）。既存の ADR の変更は数えない                                                                                                                                                                                                                                                                                           | `ALLOW_MULTI_TOPIC=<理由>`（1 つの決定を複数の ADR に分けたときと、決定前の案（status: proposed）の ADR をまとめて加えるときだけ。マージのメッセージに理由を書く） |
| `pre-pr-create-check`       | `gh pr create` の `--title`（70 文字以内）、`--body` / `--body-file`（`## Summary` / `## Changes` / `## Tests`）、未プッシュコミットなし、`--web` なし、新しい ADR が 2 件以上でない（`pre-git-merge-check` と同じ判定。base は `--base`、無ければ `gh-merge-base` の設定、それも無ければ既定ブランチ）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | 新しい ADR の検査だけ `ALLOW_MULTI_TOPIC=<理由>`（条件は `pre-git-merge-check` と同じ）                                                                            |
| `require-background-wait`   | `gh-wait-review.sh` / `request-rereview.sh` の `run_in_background=true`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | なし                                                                                                                                                               |
| `guard-sbatch`              | `sbatch` の直接の投入（`slurm-run.sh` に寄せる）と、`slurm-run.sh` のフォアグラウンド実行                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | `ALLOW_RAW_SBATCH=<理由>`（直接の投入）、`ALLOW_FOREGROUND_SLURM=<理由>`（フォアグラウンド）                                                                       |
| `delegate-request-fields`   | （Agent ツール）空でない最初の行に「/delegate を呼び、」がある依頼（prompt、または 1 行目の「依頼文は <絶対パス> にある」が指すファイル）に、定型の 6 つの欄（リポジトリ、ブランチ、何をするか、前提、完成の形、変える範囲）の見出しがそろうこと。見出しは行頭の欄の名前に `:` か `（…）:` が続く行（ADR-0061）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | なし（修正方法は理由に示す）                                                                                                                                       |
| `warn-large-commit`         | （ゲートではない）ステージ済みが 5 ファイル以上または 100 行以上なら件数を `additionalContext` で伝える。分割するかの判断はモデルに残す                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | —                                                                                                                                                                  |

hook はコマンド文字列を正規表現ではなく tree-sitter-bash で構文解析して判定する
（`claude-hooks/src/shell.rs`）。ヒアドキュメントの本文（`heredoc_body`）はデータであって
コマンドではないので配下を走査しない。本文の中身を読む検査（PR 本文・マージコミット本文の見出し、
GraphQL の mutation）は、その引数ノードの原文（入れ子のヒアドキュメント本文を含む）だけを見るので、
別の場所に見出しの例を書いてもゲートは通らない。クォートの中の `<<` や `-m "git add -A"` のような
文字列は AST 上コマンドではないため反応しない。ラッパー（`env`、`sudo`、`xargs`、`flock`、`bash [opts] <script>` など。
`src/unwrap.rs` の表）はほどき、`bash -c '…'`、`eval`、`watch`、`ssh <接続先> …` の文字列は深さ 5 まで再帰的に
解析する。実行するものが決まらない形（`$CMD`、`source`、`bash -s`）は、対象語が文字列にあれば止める側に倒す
（ADR-0025）。対象ディレクトリは同じコマンドの `git -C <dir>` と、
そのコマンドより前にある `cd <dir>` で決める。新しいルールは `src/rules/` に 1 ファイルで追加し、
`rules/mod.rs` の一覧に載せる（テストは `tests/hooks.sh` に `--rule <name>` で 1 ルールずつ書く）。

git 側の hook（`modules/home/parts/git.nix`、`core.hooksPath` でグローバル配布）も同じ考え方で、
Claude Code セッション（`CLAUDECODE=1`）のコミットに対して pre-commit が main 直接コミットの
拒否、機密情報の検査（gitleaks。ステージした中身を見る。行の `gitleaks:allow`、`.gitleaksignore`、
`.gitleaks.toml` で除き、`ALLOW_SECRET_COMMIT=<理由>` で迂回する。ADR-0015）と言語別チェック・Markdown 自動修正を、
commit-msg が Conventional Commits の形式検証を行う。
pre-merge-commit は main / master へのローカルのマージ（GitHub リモートのないリポジトリ）を、
マージ結果が `run-checks.sh --merge` を通ったときだけコミットさせる（セッションを問わない）。
`gh pr merge` は GitHub 側のマージなので、CI と `pre-merge-check` が守る。
post-rewrite はゲートではなく、`git config remap.commitRefs true` でオプトインしたリポジトリで、
rebase と amend の後に `commit-refs remap`（`modules/home/parts/commit-refs`、ADR-0012）を呼び、
文書の中の変わったコミットの番号を付け直す（コミットはしない）。git filter-repo の後は
`commit-refs rewrite-history .git/filter-repo/commit-map` で、履歴の中の文書と本文の番号も付け直す（ADR-0013）。
履歴の中の崩れた語は `history-rewrite`（`modules/home/parts/history-rewrite`、ADR-0045）が同じ辞書で直し、番号を
`commit-refs rewrite-history` で付け直す。生きているリポジトリに移す手順は `skills/history-rewrite/` にある。

**PR フローのゲートは GitHub リモートのあるリポジトリだけに効く**。リモートに `github.com` がなければ、
`block-main-commit` と `guard-git-push` は何もせず、git の pre-commit は main 直接コミットの拒否**だけ**を
飛ばす（言語別チェックと Markdown 自動修正はリモートの有無に関係なく走る）。
PR を作れないリポジトリで feature branch を強制すると、マージする手段がないまま作業が止まるため。
GitHub 以外のリモートは扱わない前提（無い場合と同じ扱い）。

**2 つの層で同じルールを守るときは例外も揃える**。`block-main-commit` と git の pre-commit は
どちらも main への直接コミットを拒否するが、`flake.lock` だけの更新（`nix-rebuild update` の正規フロー）と
GitHub リモートなしは両方で例外にする。片方だけが塞ぐと作業が止まる（PR #165）。
`modules/home/parts/tests/main-commit-gates.sh` が両層の判定の一致を検証する。
また、リポジトリ内で自動コミットするスクリプトの件名は commit-msg hook を通る形式にする必要があり、
`modules/home/parts/tests/commit-msg-conventions.sh` がその整合を検証する
（スクリプトの `git commit -m` を集めて hook 自身に通す）。

判定できないルール（1 コミット 1 論理変更、TDD の分離、指摘の要否判断）は SKILL.md に残す。

## スクリプトが担う手順

| script                                  | 元の SKILL.md の手順                                                                                                                                                                                                                                    | 呼び出すスキル                                  |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| `scripts/git-info.sh`                   | 状態の収集・整形、マージ済みブランチの worktree 検出                                                                                                                                                                                                    | git-info                                        |
| `scripts/worktree-add.sh`               | `../<repo>-<branch>` 命名で worktree 作成、未コミット変更の持ち込み、`.worktreelink` の共有                                                                                                                                                             | git-worktree, git-commit, delegate              |
| `scripts/branch-topics.sh`              | ブランチの ADR・issue の追加と status の変化、型別のコミット数                                                                                                                                                                                          | gh-pr-create, gh-pr-merge                       |
| `scripts/rename-branch.sh`              | feature ブランチのリネーム（リモート更新は `--remote` で明示）                                                                                                                                                                                          | git-branch                                      |
| `scripts/rename-plan.sh`                | `docs/plans/` のランダム名計画書を `NNN_name.md` に                                                                                                                                                                                                     | git-branch                                      |
| `scripts/git-merge-state.sh`            | 対象の検証（main 自身・GitHub リモート・未コミット）と、BEHIND・STACKED_ON・worktree・PROPOSED_ADRS の収集                                                                                                                                              | git-merge                                       |
| `scripts/check-each-commit.sh`          | `<base>..HEAD` の各コミットを一時 worktree で検査コマンドにかけ、`hash 件名 exit=N` を出す                                                                                                                                                              | delegate                                        |
| `scripts/review-level.sh`               | ローカルの `/code-review` のレベル（docs/ だけなら medium、他は high）と対象ブランチと worktree のパスの決定                                                                                                                                            | gh-pr-create, gh-pr-review, git-merge, delegate |
| `scripts/base-moved.sh`                 | 比較元が分岐点から進み、ブランチと同じパスを触っているときの通知（GitHub の origin を fetch、deny しない）                                                                                                                                              | gh-pr-create, delegate                          |
| `scripts/post-merge-cleanup.sh`         | 未マージなら止まる → worktree 削除 → main 最新化と rebuild・claude-sync の要否の通知 → 予約のタグの解放 → ブランチ削除                                                                                                                                  | gh-pr-merge, git-merge                          |
| `scripts/gh-actions-diagnose.sh`        | run 取得・失敗ジョブ特定・エラー抽出・原因分類（`CAUSE:`）                                                                                                                                                                                              | gh-actions-check                                |
| `scripts/gh-wait-review.sh`             | レビュー到着の待機（基準は最後のレビュー要求）                                                                                                                                                                                                          | gh-pr-create/merge/review, delegate             |
| `scripts/gh-review-requests.sh`         | Copilot 宛てレビュー要求の時刻を発生順に出す                                                                                                                                                                                                            | gh-wait-review, gh-pr-review                    |
| `scripts/lib/pr-url.sh`                 | PR の URL の解析と `GH_REPO` の設定（source して使う。ADR-0032）                                                                                                                                                                                        | PR のスクリプト、gh-actions-diagnose            |
| `scripts/slurm-history.sh`              | jobcomp（`/var/log/slurm/job_completions`）から、ジョブの見積もり・実績・比率を表にする（`--by-name` でジョブ名ごと）。割り込みを入れた後のジョブは、時間を記録する `--timed` のものだけを表に入れる                                                    | slurm-run.sh のジョブの振り返り                 |
| `scripts/slurm-queue.sh`                | 待ち行列を、slurm-run.sh がコメントに入れた区分・参照・目的つきで走る順に並べる。`--order [--first <番号>]` は main を区分の順に並べる番号の列（slurm-run.sh が投げるたびに `scontrol top` に渡す）。外で崩れたときは `ORDER:` で `scontrol top` を示す | slurm-run                                       |
| `language-checks/scripts/run-checks.sh` | 言語検出とフォーマット → リント → テストの実行。リポジトリの `scripts/run-python-tests.sh` と `scripts/run-rust-docs-tests.sh`（差分が `.md` だけなら全体の代わりに、ほかの差分では全体の後に実行する Rust のテスト）を読む                             | gh-pr-review（language-checks 経由）, delegate  |
| `gh-pr-review/scripts/*.sh`             | レビューコメントの取得・返信・resolve・次の行動判定                                                                                                                                                                                                     | gh-pr-review, delegate                          |
| `retro/scripts/*.sh`                    | 会話の記録からの拒否・失敗の集計、メモリの索引と実ファイルの突き合わせ                                                                                                                                                                                  | retro                                           |

### worktree の間で共有するパス（`.worktreelink`）

`worktree-add.sh` は、新しい worktree のルートに `.worktreelink` があれば、そこに 1 行 1 パスで書いたパスを
メイン worktree の同じパスへの絶対パスの symlink にし、`SHARED: <path>` を出す。git で追跡しない大きなデータ
（キャッシュ・参照データ）をコピーせず共有するため。名前は PostHog Desktop の `.worktreelink`
（[PostHog Desktop worktrees](https://posthog.com/docs/posthog-desktop/worktrees)）から借りたが、書き方は
リテラルの相対パスだけに絞った（`.gitignore` のパターンは受け付けない）。決定は ADR-0016。

```text
# 核データと DVC のキャッシュ
references
```

- 共有するパスは git に無視されていること。`dir/` のようにディレクトリだけに一致するパターンでは symlink は
  無視されないので、`dir` か `/dir` と書く。無視されていなければエラーで止まる（symlink は 1 つも作らず、worktree は残る）
- メイン worktree に無いパスは作らずに `NOTE:` で知らせる。メイン worktree で追跡されているパス、同じパスや入れ子の
  パス、pathspec として特別な意味を持つ書き方（`:` `!` で始まる、`*` `?` `[` `\` を含む）はエラー
- `post-merge-cleanup.sh` は symlink の残る worktree もそのまま消せ、先の実体は残る

## スクリプトの書き方

- `set -euo pipefail`。失敗を黙って 0 件扱いにしない（する場合はコメントで明記）
- **スクリプトから別のスクリプトを呼ぶ**ときは自身の位置から相対で解決する
  （`$HOME/.claude` 固定はリポジトリから実行したとき未同期の旧版を呼ぶ）
- **SKILL.md の手順からスクリプトを呼ぶ**ときはデプロイ先の `~/.claude/...` を使う。
  スキルは任意のプロジェクトのカレントディレクトリで実行されるため、
  `./.claude/...` は nixfiles 以外では存在しない
- Bash ツールから呼ぶスクリプトは `claude-code.nix` の許可リストに追加する
  （反映は activation なので rebuild が必要。`claude-sync` では反映されない）
- 結果は `KEY: value` 形式の行で出す（`WORKTREE:` / `CAUSE:` / `VERDICT:` 等）。
  SKILL.md はその行を読んで分岐する
- shellcheck を通す（`run-checks.sh` がステージ済みの `.sh` を `shellcheck -S warning` で検査する）
- hook / script / SKILL.md を変えたら `bash .claude/tests/{hooks,scripts,skills}.sh` を通す
  （hooks.sh は `cargo build` してからバイナリを `--rule` でルールごとに評価する。構文解析の単体テストは
  `cargo test --manifest-path modules/home/parts/claude-hooks/Cargo.toml`）。
  テストは一時 HOME と偽の `gh` / `gitleaks` / 言語ツールで隔離実行され、実環境の設定・認証を参照しない。
  TDD（テストを先にコミットしてから実装）で進める
- claude-hooks のコマンドの読み方（`shell.rs`、`unwrap.rs`、`git_args.rs`）やルールを書き直したら、PR の前に
  `bash .claude/tests/hooks-diff.sh [BASE]`（既定 origin/main）で、古い版と判定を比べる（2 つの版の nix build と
  1 分あまりの比較。1 コアの処理なので、Slurm に流さず手元で実行する）。`LOOSENED` の行（古い版が
  止め、新しい版が通す）は、意図した変更か後退かを 1 つずつ確かめ、件数と理由を PR の Tests に書く。テストが
  書いた形しか見ないのに対し、ラッパー・ディレクトリの移り方・コマンドを組み合わせた集まりで、ルールごとに比べる（ADR-0024）
