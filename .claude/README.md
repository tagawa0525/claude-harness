# .claude の構成

Claude Code のグローバル設定。`claude-sync`（または home-manager の activation）で
`~/.claude` に同期される（同期ポリシーは `modules/home/scripts/claude-sync.sh`）。

## スクリプトの置き場所

| 場所                              | 用途                                                                                                                            | 例                                                               |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| `hooks/`                          | Claude Code の hook（PreToolUse 等）。`modules/home/parts/claude-code.nix` の `claudeGlobalHooks` で settings.json に登録される | `block-main-commit.sh`                                           |
| `scripts/`                        | 複数スキルから呼ばれる共有スクリプト                                                                                            | `gh-wait-review.sh`（gh-pr-create / gh-pr-merge / gh-pr-review） |
| `skills/<name>/scripts/`          | そのスキル専用のスクリプト                                                                                                      | `gh-pr-review/scripts/decide-next.sh`                            |
| `skills/language-checks/scripts/` | 言語別の品質チェック・自動修正ツール。language-checks を参照する全スキル（git-commit / gh-pr-review 等）から使う                | `fix-markdown-lint.py`                                           |

判断基準:

- 1 つのスキルだけが使う → `skills/<name>/scripts/`
- 言語やファイル種別に対するチェック・修正 → `skills/language-checks/scripts/`
  （呼び出し元がスキルであってもここに置く。使う側ではなく対象で分類する）
- 上記以外で複数スキルが使う → `scripts/`
- Claude Code が settings.json 経由で起動するもの → `hooks/`

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
