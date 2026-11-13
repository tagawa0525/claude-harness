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
| `modules/home/parts/claude-setup/`                                       | `~/.claude` への同期と `settings.json` の更新（Linux の activation と Windows の設置が呼ぶ）    |
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

## Windows で使う

ネイティブの Windows（WSL を使わない）では、Nix の代わりに `cargo install` と `claude-setup` で入れる（ADR-0084）。
今入るのは、`~/.claude` の中身（CLAUDE.md、スキル、スクリプト）、settings.json の静的な設定、PreToolUse hook と、
現在時刻の UserPromptSubmit hook。git hook と Stop の hook（自動メモリの退避）はまだ入らない（#101 の段階 4 と 8）。
git hook が無いので、コミットとマージのメッセージの検査（Conventional Commits、件名の長さ、日本語の語）も、
PreToolUse hook が飛ばして走らない（hook はグローバルの `core.hooksPath` を使うリポジトリだけを検査する）。

前もって、Git for Windows、rustup、MSVC の toolchain が使う Visual Studio の Build Tools（C++ の workload。linker と
Windows SDK）、gh、jq、[gitleaks](https://github.com/gitleaks/gitleaks) を入れ、PATH に置く。入れたら、PATH が反映されるよう
Git Bash を開き直す:

```bash
winget install Git.Git Rustlang.Rustup GitHub.cli jqlang.jq Gitleaks.Gitleaks
winget install Microsoft.VisualStudio.2022.BuildTools \
  --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

PreToolUse hook は、git の hook の無いリポジトリの `git commit` を gitleaks で調べ、gitleaks が無いとすべての `git commit` を
止める。jq は、スキルが呼ぶ sh のスクリプト（`gh-pr-review/scripts/` など。#101 の段階 5 で Rust に移す）が使い、
Git for Windows には入っていない。

Git Bash で、clone を作るか更新してから、crate を入れ、clone のパスを渡して設置する:

```bash
git clone https://github.com/tagawa0525/claude-harness ~/nix/claude-harness  # 2 回目からは git -C ~/nix/claude-harness pull --ff-only
cd ~/nix/claude-harness
for crate in claude-hooks ja-terms claude-setup; do
  cargo install --locked --force --path "modules/home/parts/$crate"
done
claude-setup sync ~/nix/claude-harness
claude-setup settings --static modules/home/parts/claude-settings.json \
  --pre-tool-use "$(cygpath -w "$(command -v claude-hooks.exe)")"
```

clone を更新したら、同じ手順をもう一度流す（crate の版は変えないので `--force` で入れ直す）。Windows の CI
（`.github/workflows/windows.yml` の `setup`）が、この手順を流し、登録した hook が main への `git commit` を止めることを
確かめる。

## 経緯

2026-11-12 に nixfiles から切り出した。履歴はハーネスのパスだけを残して引き継いでいる。切り出しの決定は nixfiles の
`docs/adr/0082-split-claude-harness-into-its-own-repository.md`。Slurm のクラスタの構成など、PC の構成の決定は nixfiles の
`docs/adr/` にある。
