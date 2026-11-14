# Windows で使う

ネイティブの Windows（WSL を使わない）では、Nix の代わりに `cargo install` と `claude-setup` で入れる（ADR-0084）。
今入るのは、`~/.claude` の中身（CLAUDE.md、スキル、スクリプト）、settings.json の静的な設定、PreToolUse hook、
現在時刻の UserPromptSubmit hook、自動メモリを退避する Stop の hook、グローバルの git hook（pre-commit、pre-merge-commit、
commit-msg、post-rewrite。`~/.config/git/hooks` に置き、グローバルの `core.hooksPath` を向ける）。グローバルの
`core.hooksPath` をほかの置き場にしていれば、`claude-setup git-hooks` は奪わずに止まる。

前もって、Git for Windows、rustup、MSVC の toolchain が使う Visual Studio の Build Tools（C++ の workload。linker と
Windows SDK）、gh、[gitleaks](https://github.com/gitleaks/gitleaks) を入れ、PATH に置く。入れたら、PATH が反映されるよう
Git Bash を開き直す:

```bash
winget install Git.Git Rustlang.Rustup GitHub.cli Gitleaks.Gitleaks
winget install Microsoft.VisualStudio.2022.BuildTools \
  --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

PreToolUse hook は、git の hook の無いリポジトリの `git commit` を gitleaks で調べ、gitleaks が無いとすべての `git commit` を
止める。スキルと hook が呼ぶ sh のスクリプトは jq を使わない（前の手順で入れた jq は、残しても外してもよい）。
Windows でハーネスのテストの sh（`modules/home/parts/claude-hooks/tests/*.sh` など）を流すときだけ jq が要る。

Git Bash で、clone を作るか更新してから、crate を `~/.claude/bin` に入れ、clone のパスを渡して設置する。Linux と同じ
`~/.claude/bin/claude-hooks` に置くのは、スキルの sh のスクリプトがこのパスで呼ぶため（ADR-0087）。`ja-terms` と
`commit-refs` は、`claude-hooks` が隣のものを起動する（Windows の起動は、PATH の前に起動した側の置き場を探す）。
`history-rewrite`（スキル history-rewrite が名前で呼ぶ）は、PATH の `commit-refs` を呼ぶ:

```bash
git clone https://github.com/tagawa0525/claude-harness ~/nix/claude-harness  # 2 回目からは git -C ~/nix/claude-harness pull --ff-only
cd ~/nix/claude-harness
for crate in claude-hooks ja-terms commit-refs claude-setup history-rewrite; do
  cargo install --locked --force --root ~/.claude --path "modules/home/parts/$crate"
done
~/.claude/bin/claude-setup sync ~/nix/claude-harness
~/.claude/bin/claude-setup settings --static modules/home/parts/claude-settings.json \
  --pre-tool-use "$(cygpath -w ~/.claude/bin/claude-hooks.exe)" --stop "$(cygpath -w ~/.claude/bin/claude-hooks.exe)"
~/.claude/bin/claude-setup git-hooks --bin "$(cygpath -w ~/.claude/bin/claude-hooks.exe)"
```

`~/.claude/bin` に初めて入れたとき（前の手順の `~/.cargo/bin` から移るときを含む）は、Git Bash の `~/.bashrc` で `~/.claude/bin` を PATH の先頭に足し、Git Bash を開き直す（Claude Code の
Bash ツールも `~/.bashrc` を読む）。スキルと hook の修正方法の文は `claude-setup` と `commit-refs` を名前で呼ぶ。前の手順で
`~/.cargo/bin` に入れていたら、古い版が残らないよう外す:

```bash
grep -qsxF 'export PATH="$HOME/.claude/bin:$PATH"' ~/.bashrc || echo 'export PATH="$HOME/.claude/bin:$PATH"' >>~/.bashrc
cargo uninstall claude-hooks ja-terms commit-refs claude-setup  # ~/.cargo/bin に入れていたものだけを並べる
```

開き直した Git Bash で `command -v claude-setup` が `~/.claude/bin/claude-setup` を示すことを確かめる（Git Bash の
ログインシェルは `~/.bash_profile` を読み、Git for Windows の作る `~/.bash_profile` が `~/.bashrc` を読む）。

Stop の hook（`claude-hooks stop`）は、応答の終わりごとに自動メモリ（`~/.claude/projects/*/memory/`）を `~/.claude` の
git の ref `refs/memory-backup` にコミットする。`~/.claude` が git のリポジトリの直下でなければ、応答のたびに退避の失敗を
通知する。`claude-setup` はリポジトリを作らない（`.gitignore` の無いまま作ると、後の `git add -A` で `.credentials.json`
や会話の記録が入りうる。上の階層のリポジトリの下に作るかは、そのリポジトリとの兼ね合いで決める。ADR-0076）。退避は
`git add -f` でメモリだけを入れるので、何も追跡しないリポジトリでも動く（コミットには git の `user.name` と `user.email`
が要る）。`git -C ~/.claude rev-parse --show-toplevel` が「not a git repository」で失敗するときは、`.gitignore` と一緒に
作る（既にある `.gitignore` は上書きしない）。`.gitignore` は、同期した CLAUDE.md、スキル、スクリプトと自動メモリだけを
除外から外す。すべてを除外（`/*`）すると、Claude Code の Grep（ripgrep）が `.gitignore` に従って、`~/.claude` の中の
スキルも自動メモリも見つけなくなる（実機の Windows で確かめた）。`.credentials.json`、会話の記録、`bin` は除外のまま残る:

```bash
git init -q -b main ~/.claude
[ -e ~/.claude/.gitignore ] || printf '%s\n' '/*' '!/.gitignore' '!/CLAUDE.md' '!/skills/' '!/scripts/' \
  '!/projects/' '/projects/*/*' '!/projects/*/memory/' >~/.claude/.gitignore
```

`cargo install --root ~/.claude` は、入れた crate の記録（`.crates.toml`、`.crates2.json`）を `~/.claude` に置く。
`--force` で入れ直すときに使うので消さない。

clone を更新したら、同じ手順をもう一度流す（crate の版は変えないので `--force` で入れ直す）。Windows の CI
（`.github/workflows/windows.yml` の `setup`）が、この手順を流し、登録した PreToolUse hook と git hook が main への
`git commit` を止めること、Stop の hook がメモリを退避することを確かめる。
