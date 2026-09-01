# .claude の構成

Claude Code のグローバル設定。`claude-sync`（または home-manager の activation）で
`~/.claude` に同期される（同期ポリシーは `modules/home/scripts/claude-sync.sh`）。

## スクリプトの置き場所

| 場所                              | 用途                                                                                                                            | 例                                                                                              |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `hooks/`                          | Claude Code の hook（PreToolUse 等）。`modules/home/parts/claude-code.nix` の `claudeGlobalHooks` で settings.json に登録される | `block-main-commit.sh`, `guard-git-push.sh`, `pre-merge-check.sh`, `require-background-wait.sh` |
| `scripts/`                        | 複数スキルから呼ばれる共有スクリプト                                                                                            | `gh-wait-review.sh`（gh-pr-create / gh-pr-merge / gh-pr-review）                                |
| `skills/<name>/scripts/`          | そのスキル専用のスクリプト                                                                                                      | `gh-pr-review/scripts/decide-next.sh`                                                           |
| `skills/language-checks/scripts/` | 言語別の品質チェック・自動修正ツール。language-checks を参照する全スキル（git-commit / gh-pr-review 等）から使う                | `fix-markdown-lint.py`                                                                          |

判断基準:

- 1 つのスキルだけが使う → `skills/<name>/scripts/`
- 言語やファイル種別に対するチェック・修正 → `skills/language-checks/scripts/`
  （呼び出し元がスキルであってもここに置く。使う側ではなく対象で分類する）
- 上記以外で複数スキルが使う → `scripts/`
- Claude Code が settings.json 経由で起動するもの → `hooks/`

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

| hook                         | 強制するルール                                                                                             | エスケープ                                   |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `block-main-commit.sh`       | main / master での `git commit`                                                                            | なし                                         |
| `guard-git-push.sh`          | main / master への push、force push（`--force-with-lease` は feature branch のみ可）、`--all` / `--mirror` | コマンドに `ALLOW_PROTECTED_PUSH=1` を付ける |
| `pre-merge-check.sh`         | `gh pr merge` の `--merge` / `--delete-branch` / 本文見出し、CI、reviewDecision、未解決スレッド            | なし                                         |
| `require-background-wait.sh` | `gh-wait-review.sh` / `request-rereview.sh` の `run_in_background=true`                                    | なし                                         |

git 側の hook（`modules/home/parts/git.nix`、`core.hooksPath` でグローバル配布）も同じ考え方で、
Claude Code セッション（`CLAUDECODE=1`）のコミットに対して pre-commit が main 直接コミットの
拒否と言語別チェック・Markdown 自動修正を、commit-msg が Conventional Commits の形式検証を行う。

判定できないルール（1 コミット 1 論理変更、TDD の分離、指摘の要否判断）は SKILL.md に残す。

## スクリプトの書き方

- `set -euo pipefail`。失敗を黙って 0 件扱いにしない（する場合はコメントで明記）
- **スクリプトから別のスクリプトを呼ぶ**ときは自身の位置から相対で解決する
  （`$HOME/.claude` 固定はリポジトリから実行したとき未同期の旧版を呼ぶ）
- **SKILL.md の手順からスクリプトを呼ぶ**ときは配備先の `~/.claude/...` を使う。
  スキルは任意のプロジェクトのカレントディレクトリで実行されるため、
  `./.claude/...` は nixfiles 以外では存在しない
- Bash ツールから呼ぶスクリプトは `claude-code.nix` の許可リストに追加する
  （反映は activation なので rebuild が必要。`claude-sync` では反映されない）
- shellcheck を通す: `nix shell nixpkgs#shellcheck -c shellcheck -S warning <file>`
