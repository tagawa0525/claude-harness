---
type: Guide
title: Windows で使う
description: ネイティブの Windows（WSL を使わない）で、cargo install と claude-setup でハーネスを入れる手順。
---

# Windows で使う

ネイティブの Windows（WSL を使わない）では、Nix の代わりに `cargo install` と `claude-setup` で入れる（ADR-0084）。
今入るのは、`~/.claude` の中身（CLAUDE.md、スキル、スクリプト）、settings.json の静的な設定、PreToolUse hook、
現在時刻と既定ブランチの進みを出す UserPromptSubmit hook、自動メモリを退避する Stop の hook、グローバルの git hook（pre-commit、pre-merge-commit、
commit-msg、post-rewrite。`~/.config/git/hooks` に置き、グローバルの `core.hooksPath` を向ける）、改行を LF にそろえる
グローバルの git の attributes（`config/git/attributes` を `~/.config/git/attributes` に置く。ADR-0105）。グローバルの
`core.hooksPath` をほかの置き場にしていれば、`claude-setup git-hooks` は奪わずに止まる。`core.attributesFile` がほかの
ファイルを指すか、`~/.config/git/attributes` に自分のファイルがあれば、`claude-setup git-attributes` は奪わずに止まる。

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
Windows でハーネスのテストの sh（`crates/claude-hooks/tests/*.sh` など）を流すときだけ jq が要る。

前から Git for Windows を入れている機械では、版を上げる（`winget upgrade Git.Git`）。`commit-refs` の履歴の書き換え
（スキル history-rewrite が使う）は `git fast-export --signed-commits` を渡し、Git 2.47.1 はこのオプションを知らずに
使い方を出して終わった（2.56.1 では動いた。実機の Windows、2026-11-14、#213）。

symlink を作れるよう、Windows の開発者モードを有効にする（設定の検索で「開発者モード」。置き場の名前は Windows の版で違う）。`worktree-add.sh` は、
リポジトリの `.worktreelink` に書いたパスをメインの worktree への symlink にする。開発者モードが無く管理者でもないと、
symlink を作れずに失敗する（`claude-hooks` の `cargo test` も 1 件が失敗した。#213）。

Git Bash で、clone を作るか更新してから、`scripts/install-windows.sh` を流す。スクリプトは、crate を `~/.claude/bin` に
入れ、clone を同期し、settings.json の hook と git hook と git の attributes を置き、`~/.bashrc` の PATH と、下の `~/.claude` の退避のリポジトリを
作る。clone の場所はスクリプトの位置から決めるので、`~/nix/claude-harness` 以外に clone した機械でも同じに流せる。
何度流してもよい:

```bash
git clone https://github.com/tagawa0525/claude-harness ~/nix/claude-harness  # 2 回目からは git -C ~/nix/claude-harness pull --ff-only
bash ~/nix/claude-harness/scripts/install-windows.sh
```

以下は、スクリプトが何をなぜするか。Linux と同じ
`~/.claude/bin/claude-hooks` に置くのは、スキルの sh のスクリプトがこのパスで呼ぶため（ADR-0087）。`ja-terms` と
`commit-refs` は、`claude-hooks` が隣のものを起動する（Windows の起動は、PATH の前に起動した側の置き場を探す）。
`history-rewrite`（スキル history-rewrite が名前で呼ぶ）は、PATH の `commit-refs` を呼ぶ。`design-records` は、スキル adr と git-merge、
`run-checks.sh` の ADR の検査、`post-merge-cleanup.sh` の番号の予約の解放が名前で呼ぶ（無いと、2 つのスクリプトは検査と解放を
飛ばし、スキルの手順のコマンドは見つからずに止まる）。

Markdown の検査（`run-checks.sh`、pre-commit）が呼ぶ `markdownlint` は npm で入れる（`npm install -g markdownlint-cli`。無いと検査は
SKIP になる）。設定を持たないリポジトリで効くグローバルの設定は、`claude-setup sync` が `config/markdownlint/config` から
`~/.config/markdownlint/config` に置く。Windows の `%USERPROFILE%` の下で markdownlint がこの場所を読むかは、実機で確かめていない（ADR-0093）。

崩れた日本語の語の共通の辞書も、`claude-setup sync` が `crates/ja-terms/prh.yml` から `%USERPROFILE%\.config\ja-terms\prh.yml` に
置き、`ja-terms` はそこを読む（ADR-0097。ビルドに埋めない）。スクリプトが `claude-setup sync` を流す前は、`ja-terms` が辞書が無い
として検査を止め、コミットが通らない。辞書の規則だけが変わったときは、clone を更新して `claude-setup sync` を流せば効く。

スクリプトは、Git Bash の `~/.bashrc` で `~/.claude/bin` を PATH の先頭に足す（Claude Code の Bash ツールも `~/.bashrc` を
読む）。足したときはそう知らせるので、Git Bash を開き直す。スキルと hook の修正方法の文は `claude-setup` と `commit-refs` を
名前で呼ぶ。前の手順で `~/.cargo/bin` に入れていたら、古い版が残らないよう外す（スクリプトはしない）:

```bash
cargo uninstall claude-hooks ja-terms commit-refs claude-setup  # ~/.cargo/bin に入れていたものだけを並べる
```

開き直した Git Bash で `command -v claude-setup` が `~/.claude/bin/claude-setup` を示すことを確かめる（Git Bash の
ログインシェルは `~/.bash_profile` を読み、Git for Windows の作る `~/.bash_profile` が `~/.bashrc` を読む）。

Stop の hook（`claude-hooks stop`）は、応答の終わりごとに自動メモリ（`~/.claude/projects/*/memory/`）を `~/.claude` の
git の ref `refs/memory-backup` にコミットする。`~/.claude` が git のリポジトリの直下でなければ、応答のたびに退避の失敗を
通知する。`claude-setup` はリポジトリを作らない（`.gitignore` の無いまま作ると、後の `git add -A` で `.credentials.json`
や会話の記録が入りうる。上の階層のリポジトリの下に作るかは、そのリポジトリとの兼ね合いで決める。ADR-0076）。退避は
`git add -f` でメモリだけを入れるので、何も追跡しないリポジトリでも動く（コミットには git の `user.name` と `user.email`
が要る）。スクリプトは、`~/.claude` がリポジトリでなければ `.gitignore` と一緒に作り（既にある `.gitignore` は上書きしない）、
上の階層のリポジトリの下にあれば作らずに知らせる。`.gitignore` は、同期した CLAUDE.md、スキル、スクリプトと自動メモリだけを
除外から外す。すべてを除外（`/*`）すると、Claude Code の Grep（ripgrep）が `.gitignore` に従って、`~/.claude` の中の
スキルも自動メモリも見つけなくなる（実機の Windows で確かめた）。`.credentials.json`、会話の記録、`bin` は除外のまま残る。
退避の後に、最後の応答が英語で書かれていれば、日本語で書き直すよう応答を続けさせる（ADR-0108）。

`cargo install --root ~/.claude` は、入れた crate の記録（`.crates.toml`、`.crates2.json`）を `~/.claude` に置く。
`--force` で入れ直すときに使うので消さない。

ほかのセッションが動いていても、スクリプトを流してよい。Windows は動いている exe の上書きを拒むので、スクリプトは入っている exe を
`<名前>.exe.old-<時刻>-<pid>` にリネームしてから入れる（#414）。リネームした exe は使うプロセスが終わるまで消せないので
`~/.claude/bin` に残り、次に流したときに、使われていないものを消す。

clone を更新したら、スクリプトをもう一度流す（crate の版は変えないので `--force` で入れ直す）。Windows の CI
（`.github/workflows/windows.yml` の `setup`）が、スクリプトを流し、登録した PreToolUse hook と git hook が main への
`git commit` を止めること、Stop の hook がメモリを退避することを確かめる。
