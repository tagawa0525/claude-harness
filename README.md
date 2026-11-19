# claude-harness

Claude Code のハーネス。PC 全体のセッションが読む指示、スキル、スクリプトと、決まりを機械的に守らせる hook、それらが使う CLI を
持つ。NixOS と home-manager の構成（[nixfiles](https://github.com/tagawa0525/nixfiles)）が flake input として取り込む。

## 中身

| 場所                                                                       | 何があるか                                                                                                                       |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `CLAUDE.md`、`.claude/skills/`                                             | このリポジトリだけの規則とスキル（配備の /harness-deploy、導入の /harness-install。このリポジトリのセッションが読み、配らない）  |
| `claude/`                                                                  | グローバルの指示（`CLAUDE.md`）、スキル、スクリプト、それらのテスト。`~/.claude` に配る元（ADR-0094）。構成は `claude/README.md` |
| `config/`                                                                  | `~/.config` に配る元（markdownlint のグローバルの設定。ADR-0093）                                                                |
| `nix/claude-code.nix`                                                      | `~/.claude` への同期、`settings.json`、PreToolUse hook の登録                                                                    |
| `nix/git-hooks.nix`                                                        | git のグローバルの hook（pre-commit、pre-merge-commit、commit-msg、post-rewrite）                                                |
| `crates/claude-hooks/`                                                     | PreToolUse hook（Rust 製の 1 バイナリ）                                                                                          |
| `crates/claude-setup/`                                                     | `~/.claude` への同期と `settings.json` の更新（Linux の activation と Windows の設置が呼ぶ）                                     |
| `crates/commit-refs/`、`crates/design-records/`、`crates/history-rewrite/` | hook とスキルが使う CLI                                                                                                          |
| `crates/ja-terms/`                                                         | 日本語の語の辞書と検査                                                                                                           |
| `nix/tests/`                                                               | Nix でビルドした hook を流す sh のテスト                                                                                         |
| `claude-settings.json`                                                     | `claude-setup settings` が `~/.claude/settings.json` に書く静的な設定（Linux の activation と Windows の設置が渡す）             |
| `docs/`                                                                    | OKF のバンドル（`docs/adr/` に決定の記録）。`design-records check` が `type` と `index.md` を検査                                |

## 取り込み方

```nix
inputs.claude-harness = {
  url = "github:tagawa0525/claude-harness";
  inputs.nixpkgs.follows = "nixpkgs";
  inputs.home-manager.follows = "home-manager";
};
```

home-manager の構成で `claude-harness.homeManagerModules.default` を import する。Claude Code の本体は入れないので、
取り込む側で入れる。

## Windows で使う

ネイティブの Windows（WSL を使わない）でも、Nix の代わりに `cargo install` と `claude-setup` で入れて使える（ADR-0084）。
前もって入れるものと手順は [docs/windows.md](docs/windows.md) にある。

前提:

- symlink を作れること（開発者モードか `SeCreateSymbolicLinkPrivilege`）。無いと `worktree-add` の `.worktreelink` が使えない
- Git for Windows は新しい版にする。`commit-refs` の履歴の書き換えが使う `git fast-export --signed-commits` を、2.47.1 は知らない（2.56.1 では動いた。最小の版は確かめていない）

## 経緯

2026-11-12 に nixfiles から切り出した。履歴はハーネスのパスだけを残して引き継いでいる。切り出しの決定は nixfiles の
`docs/adr/0082-split-claude-harness-into-its-own-repository.md`。Slurm のクラスタの構成など、PC の構成の決定は nixfiles の
`docs/adr/` にある。
