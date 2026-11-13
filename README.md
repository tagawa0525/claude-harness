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
今入るのは、`~/.claude` の中身（CLAUDE.md、スキル、スクリプト）、settings.json の静的な設定、PreToolUse hook、
現在時刻の UserPromptSubmit hook、グローバルの git hook（pre-commit、pre-merge-commit、commit-msg、post-rewrite。
`~/.config/git/hooks` に置き、グローバルの `core.hooksPath` を向ける）。Stop の hook（自動メモリの退避）はまだ入らない
（#101 の段階 8）。グローバルの `core.hooksPath` をほかの置き場にしていれば、`claude-setup git-hooks` は奪わずに止まる。

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

Git Bash で、clone を作るか更新してから、crate を `~/.claude/bin` に入れ、clone のパスを渡して設置する。Linux と同じ
`~/.claude/bin/claude-hooks` に置くのは、スキルの sh のスクリプトがこのパスで呼ぶため（ADR-0087）。`ja-terms` と
`commit-refs` は、`claude-hooks` が隣のものを起動する（Windows の起動は、PATH の前に起動した側の置き場を探す）:

```bash
git clone https://github.com/tagawa0525/claude-harness ~/nix/claude-harness  # 2 回目からは git -C ~/nix/claude-harness pull --ff-only
cd ~/nix/claude-harness
for crate in claude-hooks ja-terms commit-refs claude-setup; do
  cargo install --locked --force --root ~/.claude --path "modules/home/parts/$crate"
done
~/.claude/bin/claude-setup sync ~/nix/claude-harness
~/.claude/bin/claude-setup settings --static modules/home/parts/claude-settings.json \
  --pre-tool-use "$(cygpath -w ~/.claude/bin/claude-hooks.exe)"
~/.claude/bin/claude-setup git-hooks --bin "$(cygpath -w ~/.claude/bin/claude-hooks.exe)"
```

初めて入れたときは、`~/.claude/bin` を利用者の PATH の先頭に足し、Git Bash を開き直す。スキルと hook の修正方法の文は
`claude-setup` と `commit-refs` を名前で呼ぶ。前の手順で `~/.cargo/bin` に入れていたら、PATH で古い版が先に見つからない
よう外す:

```bash
powershell -NoProfile -Command '$d = "$env:USERPROFILE\.claude\bin"; $p = [Environment]::GetEnvironmentVariable("Path", "User"); if (($p -split ";") -notcontains $d) { [Environment]::SetEnvironmentVariable("Path", "$d;$p", "User") }'
cargo uninstall claude-hooks ja-terms commit-refs claude-setup  # ~/.cargo/bin に入れていたときだけ
```

`cargo install --root ~/.claude` は、入れた crate の記録（`.crates.toml`、`.crates2.json`）を `~/.claude` に置く。
`--force` で入れ直すときに使うので消さない。

clone を更新したら、同じ手順をもう一度流す（crate の版は変えないので `--force` で入れ直す）。Windows の CI
（`.github/workflows/windows.yml` の `setup`）が、この手順を流し、登録した PreToolUse hook と git hook が main への
`git commit` を止めることを確かめる。

## 経緯

2026-11-12 に nixfiles から切り出した。履歴はハーネスのパスだけを残して引き継いでいる。切り出しの決定は nixfiles の
`docs/adr/0082-split-claude-harness-into-its-own-repository.md`。Slurm のクラスタの構成など、PC の構成の決定は nixfiles の
`docs/adr/` にある。
