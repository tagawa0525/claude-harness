# claude-harness

Claude Code のハーネス。PC 全体のセッションが読む指示、スキル、スクリプトと、決まりを機械的に守らせる hook、それらが使う CLI を
持つ。NixOS と home-manager の構成（[nixfiles](https://github.com/tagawa0525/nixfiles)）が flake input として取り込む。

## 中身

| 場所                                                                     | 何があるか                                                                                      |
| ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------- |
| `.claude/`                                                               | グローバルの指示（`CLAUDE.md`）、スキル、スクリプト、それらのテスト。構成は `.claude/README.md` |
| `modules/home/parts/claude-code.nix`                                     | `~/.claude` への同期、`settings.json`、PreToolUse hook の登録                                   |
| `modules/home/parts/git-hooks.nix`                                       | git のグローバルの hook（pre-commit、pre-merge-commit、commit-msg、post-rewrite）               |
| `modules/home/parts/claude-hooks/`                                       | PreToolUse hook（Rust 製の 1 バイナリ）                                                         |
| `modules/home/parts/commit-refs/`、`design-records/`、`history-rewrite/` | hook とスキルが使う CLI                                                                         |
| `modules/home/parts/ja-terms/`                                           | 日本語の語の辞書と検査                                                                          |
| `docs/adr/`                                                              | 決定の記録                                                                                      |

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

## 経緯

2026-11-12 に nixfiles から切り出した。履歴はハーネスのパスだけを残して引き継いでいる。切り出しの決定は nixfiles の
`docs/adr/0082-split-claude-harness-into-its-own-repository.md`。Slurm のクラスタの構成など、PC の構成の決定は nixfiles の
`docs/adr/` にある。
