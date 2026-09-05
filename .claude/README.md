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

| 場所                                  | 用途                                                                                                                                                        | 例                                                                                                       |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `../modules/home/parts/claude-hooks/` | Claude Code の PreToolUse hook（Rust 製 1 バイナリ）。`claude-code.nix` がビルドして `~/.claude/bin/claude-hooks` に置き、settings.json に 1 件だけ登録する | `src/rules/block_main_commit.rs`, `src/shell.rs`                                                         |
| `scripts/`                            | 複数スキルから呼ばれる、または git/gh に対する汎用の手順                                                                                                    | `gh-wait-review.sh`, `git-info.sh`, `worktree-add.sh`, `post-merge-cleanup.sh`, `gh-actions-diagnose.sh` |
| `skills/<name>/scripts/`              | そのスキル専用のスクリプト                                                                                                                                  | `gh-pr-review/scripts/decide-next.sh`                                                                    |
| `skills/language-checks/scripts/`     | 言語別の品質チェック・自動修正ツール。language-checks を参照する全スキル（git-commit / gh-pr-review 等）から使う                                            | `run-checks.sh`, `fix-markdown-lint.py`                                                                  |
| `tests/`                              | hook のルール / scripts / skills のテスト（`claude-sync` の同期対象外）                                                                                     | `hooks.sh`, `scripts.sh`, `skills.sh`                                                                    |

判断基準:

- 1 つのスキルだけが使う → `skills/<name>/scripts/`
- 言語やファイル種別に対するチェック・修正 → `skills/language-checks/scripts/`
  （呼び出し元がスキルであってもここに置く。使う側ではなく対象で分類する）
- 上記以外で複数スキルが使う、または git/gh に対する汎用の手順 → `scripts/`
- Claude Code が settings.json 経由で起動するもの → `claude-hooks` のルール（`src/rules/` に 1 ファイル）

## 外部リポジトリ由来のスキル

`~/.claude/skills/` には、このリポジトリの `.claude/skills/` 以外に外部リポジトリの
スキルも配備される。出所は flake input として `flake.nix` / `flake.lock` に記録し、
対象は `modules/home/parts/claude-code.nix` の `externalSkills` で列挙する。

| スキル                 | 出所                                                                             |
| ---------------------- | -------------------------------------------------------------------------------- |
| `grilling`, `grill-me` | [mattpocock/skills](https://github.com/mattpocock/skills) `skills/productivity/` |

- 更新追従: `nix flake update mattpocock-skills` → rebuild
- 配備先は上流の完全なコピー（`rsync --delete`）。ローカルで編集しても rebuild で戻る
- `claude-sync` コマンドは flake input を知らないため、外部スキルは rebuild でのみ同期される

## hook が強制するルール

決定的に判定できるルールは SKILL.md の文章ではなく hook で強制する（迂回できないゲート）。

| ルール                    | 強制するルール                                                                                                                                                                               | エスケープ                                                                                         |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `block-main-commit`       | main / master での `git commit`                                                                                                                                                              | なし                                                                                               |
| `block-secret-commit`     | コミット対象に機密情報がある `git commit`。検出は gitleaks（`gitleaks git --staged`、`-a` なら `--pre-commit` も）に委ね、場所とルール ID だけを理由に出す。gitleaks が無い・失敗したら deny | 行に `gitleaks:allow`（`.gitleaksignore` / `.gitleaks.toml` も可）、または `ALLOW_SECRET_COMMIT=1` |
| `guard-git-add`           | 対象を絞らない `git add`（`-A` / `--all` / `.` / `:/` / `*`）。パスで範囲を限定した `-A src/` と `-u` は可                                                                                   | コマンドに `ALLOW_GIT_ADD_ALL=1` を付ける                                                          |
| `guard-git-push`          | main / master への push、force push（`--force-with-lease` は feature branch なら open PR があっても可）、`--all` / `--mirror`                                                                | コマンドに `ALLOW_PROTECTED_PUSH=1` を付ける                                                       |
| `guard-gh-api`            | 生の `gh api` による Actions 権限設定（`actions/permissions`、`default_workflow_permissions`）への書き込みと、GraphQL の `resolveReviewThread`（resolve は `resolve-thread.sh` 経由に固定）  | なし                                                                                               |
| `guard-gh-run-rerun`      | 既に attempt 2 以上の run への `gh run rerun`（一時障害の再実行は 1 回まで。確認できなければ deny）                                                                                          | コマンドに `ALLOW_RERUN=1` を付ける                                                                |
| `pre-merge-check`         | `gh pr merge` の `--merge` / `--delete-branch` / 本文見出し、CI、reviewDecision、未解決スレッド、head が base より遅れていない（リベース済み）                                               | なし                                                                                               |
| `pre-pr-create-check`     | `gh pr create` の `--title`（70 文字以内）、`--body` / `--body-file`（`## 概要` / `## 変更点` / `## テスト`）、未プッシュコミットなし、`--web` なし                                          | なし                                                                                               |
| `require-background-wait` | `gh-wait-review.sh` / `request-rereview.sh` の `run_in_background=true`                                                                                                                      | なし                                                                                               |
| `warn-large-commit`       | （ゲートではない）ステージ済みが 5 ファイル以上または 100 行以上なら件数を `additionalContext` で伝える。分割するかの判断はモデルに残す                                                      | —                                                                                                  |

hook はコマンド文字列を正規表現ではなく tree-sitter-bash で構文解析して判定する
（`claude-hooks/src/shell.rs`）。ヒアドキュメントの本文（`heredoc_body`）はデータであって
コマンドではないので配下を走査しない。本文の中身を読む検査（PR 本文・マージコミット本文の見出し、
GraphQL の mutation）は、その引数ノードの原文（入れ子のヒアドキュメント本文を含む）だけを見るので、
別の場所に見出しの例を書いてもゲートは通らない。クォートの中の `<<` や `-m "git add -A"` のような
文字列は AST 上コマンドではないため反応しない。対象ディレクトリは同じコマンドの `git -C <dir>` と、
そのコマンドより前にある `cd <dir>` で決める。新しいルールは `src/rules/` に 1 ファイルで追加し、
`rules/mod.rs` の一覧に載せる（テストは `tests/hooks.sh` に `--rule <name>` で 1 ルールずつ書く）。

git 側の hook（`modules/home/parts/git.nix`、`core.hooksPath` でグローバル配布）も同じ考え方で、
Claude Code セッション（`CLAUDECODE=1`）のコミットに対して pre-commit が main 直接コミットの
拒否と言語別チェック・Markdown 自動修正を、commit-msg が Conventional Commits の形式検証を行う。

**PR フローのゲートは GitHub リモートのあるリポジトリだけに効く**。リモートに `github.com` がなければ、
`block-main-commit` と `guard-git-push` は何もせず、git の pre-commit は main 直接コミットの拒否**だけ**を
飛ばす（言語別チェックと Markdown 自動修正はリモートの有無に関係なく走る）。
PR を作れないリポジトリで feature branch を強制すると、マージする手段がないまま作業が止まるため。
判定は `gh-wait-review.sh`（`SKIP: GitHubリモートがありません`）と同じで、GitHub 以外のリモートは
扱わない前提（無い場合と同じ扱い）。

**2 つの層で同じルールを守るときは例外も揃える**。`block-main-commit` と git の pre-commit は
どちらも main への直接コミットを拒否するが、`flake.lock` だけの更新（`nix-rebuild update` の正規フロー）と
GitHub リモートなしは両方で例外にする。片方だけが塞ぐと作業が止まる（PR #165）。
`modules/home/parts/tests/main-commit-gates.sh` が両層の判定の一致を検証する。
また、リポジトリ内で自動コミットするスクリプトの件名は commit-msg hook を通る形式にする必要があり、
`modules/home/parts/tests/commit-msg-conventions.sh` がその整合を検証する
（スクリプトの `git commit -m` を集めて hook 自身に通す）。

判定できないルール（1 コミット 1 論理変更、TDD の分離、指摘の要否判断）は SKILL.md に残す。

## スクリプトが担う手順

| script                                  | 元の SKILL.md の手順                                                | 呼び出すスキル                       |
| --------------------------------------- | ------------------------------------------------------------------- | ------------------------------------ |
| `scripts/git-info.sh`                   | 状態の収集・整形、マージ済みブランチの worktree 検出                | git-info                             |
| `scripts/worktree-add.sh`               | `../<repo>-<branch>` 命名で worktree 作成、未コミット変更の持ち込み | git-worktree, git-commit             |
| `scripts/rename-branch.sh`              | feature ブランチのリネーム（リモート更新は `--remote` で明示）      | git-branch                           |
| `scripts/rename-plan.sh`                | `docs/plans/` のランダム名計画書を `NNN_name.md` に                 | git-branch                           |
| `scripts/post-merge-cleanup.sh`         | worktree 削除 → main 最新化 → ローカル/リモートブランチ削除         | gh-pr-merge                          |
| `scripts/gh-actions-diagnose.sh`        | run 取得・失敗ジョブ特定・エラー抽出・原因分類（`CAUSE:`）          | gh-actions-check                     |
| `scripts/gh-wait-review.sh`             | レビュー到着の待機                                                  | gh-pr-create/merge/review            |
| `language-checks/scripts/run-checks.sh` | 言語検出とフォーマット → リント → テストの実行                      | gh-pr-review（language-checks 経由） |
| `gh-pr-review/scripts/*.sh`             | レビューコメントの取得・返信・resolve・次の行動判定                 | gh-pr-review                         |

## スクリプトの書き方

- `set -euo pipefail`。失敗を黙って 0 件扱いにしない（する場合はコメントで明記）
- **スクリプトから別のスクリプトを呼ぶ**ときは自身の位置から相対で解決する
  （`$HOME/.claude` 固定はリポジトリから実行したとき未同期の旧版を呼ぶ）
- **SKILL.md の手順からスクリプトを呼ぶ**ときは配備先の `~/.claude/...` を使う。
  スキルは任意のプロジェクトのカレントディレクトリで実行されるため、
  `./.claude/...` は nixfiles 以外では存在しない
- Bash ツールから呼ぶスクリプトは `claude-code.nix` の許可リストに追加する
  （反映は activation なので rebuild が必要。`claude-sync` では反映されない）
- 結果は `KEY: value` 形式の行で出す（`WORKTREE:` / `CAUSE:` / `VERDICT:` 等）。
  SKILL.md はその行を読んで分岐する
- shellcheck を通す: `nix shell nixpkgs#shellcheck -c shellcheck -S warning <file>`
- hook / script / SKILL.md を変えたら `bash .claude/tests/{hooks,scripts,skills}.sh` を通す
  （hooks.sh は `cargo build` してからバイナリを `--rule` でルールごとに評価する。構文解析の単体テストは
  `cargo test --manifest-path modules/home/parts/claude-hooks/Cargo.toml`）。
  テストは一時 HOME と偽の `gh` / `gitleaks` / 言語ツールで隔離実行され、実環境の設定・認証を参照しない。
  TDD（テストを先にコミットしてから実装）で進める
