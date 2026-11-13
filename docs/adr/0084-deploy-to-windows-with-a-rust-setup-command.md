---
status: proposed
date: 2026-11-12
requires: [ADR-0082, ADR-0083]
supersedes: []
superseded-by:
issues: ["#101", "#104"]
---

# ADR-0084: Windows では、手元でビルドした Rust のバイナリと、Linux と共用する Rust の設置コマンドでハーネスを配備する

## 背景

ADR-0082 は、ハーネスの hook とスクリプトを Rust で書き、ネイティブの Windows でも動かすと決め、Windows への配備を別の
ADR に回した（#101 の段階 2）。Linux の配備は Nix の home-manager が担い、Windows には Nix が無い。今の配備は次のとおり
（2026-11-12、`a1a45cf5`）。

- バイナリ: `claude-code.nix` が `claude-hooks` と `claude-memory-backup`、`ja-terms-files` を store から `~/.claude/bin/`
  に置き、`git-hooks.nix` が `design-records`、`commit-refs` などを `home.packages` で PATH に入れる
- settings.json: `claude-code.nix` の activation が jq で、静的な設定（`claudeCodeStaticSettings`、Nix の attrset）を
  重ね、hook（PreToolUse、UserPromptSubmit、Stop）の登録を 1 件ずつに揃える。旧 bash hook の登録も外す。PreToolUse の
  登録は shell form で、`command` に `$HOME` を展開した絶対パスと引数 `pre-tool-use` を 1 つの文字列で書く
- スキルと CLAUDE.md: `claude-sync.sh` が rsync で `.claude/` の CLAUDE.md、commands、skills、scripts を `~/.claude` に
  コピーする。activation も同じスクリプトを呼ぶ。flake input から取る外部のスキル（`externalSkills`）と lsp-det の
  プラグインは activation だけが置く
- git hook: `git-hooks.nix` が `~/.config/git/hooks/` に sh の hook を置き、`core.hooksPath` をそこに向ける

Windows で関わる事実:

- Windows の Claude Code は `~/.claude` を `%USERPROFILE%\.claude` と読む。`CLAUDE_CONFIG_DIR` で場所を変えられる
  （<https://code.claude.com/docs/en/settings> の "Find or create your settings files"、2026-11-12 に閲覧）
- hook の exec form（`args` を書く）は、`command` を PATH で実行ファイルとして解き、シェルを通さずに起動する。
  `$` は展開されず、展開されるのは `${CLAUDE_PROJECT_DIR}` などのパスの置き換えだけ。Windows では `command` が `.exe`
  などの実行ファイルでなければならない。shell form は Windows では Git Bash で走る
  （<https://code.claude.com/docs/en/hooks> の "Exec form and shell form"、2026-11-12 に閲覧）
- Git for Windows は git hook を同梱の sh で実行する。rsync と jq は同梱されていない
- Windows の機械には rustup と MSVC Build Tools を入れてよい（2026-11-12 のユーザーの決定）。`windows.yml`（ADR-0083）の
  `cargo test --locked` は、GitHub Actions の `windows-latest` の runner に入っている Rust の stable（MSVC の target）でビルドしている

## 決定の要因

- Windows と Linux で、hook の登録と同期の規則を 2 つの言語に書き分けないこと（ADR-0082 の要因と同じ）
- Linux の今の配備（rebuild で入れる、`claude-sync` で入れる）が作る `~/.claude` の中身と hook の判定を変えないこと
- Windows で足す仕組み（リリースの workflow、版の付け方）を、今の要件に要る分だけにすること
- 依存を増やさないこと（ADR-0047 の数え方）

## 検討した案

バイナリの入手:

- 案 1: Windows に置いた clone で `cargo install --locked --path <crate>` を実行し、手元でビルドする
- 案 2: GitHub Actions でビルドした `.exe` を GitHub Releases に置き、ダウンロードする

設置（settings.json の登録、同期、git hook）の実装:

- 案 A: 設置のコマンドを Rust で書き、Windows でも Linux（home-manager の activation と `claude-sync`）でも同じものを呼ぶ
- 案 B: 設置のコマンドを Rust で書くが、Windows でだけ使う。Linux は今の jq と rsync のまま
- 案 C: Windows では Git Bash の sh で書く
- 案 D: Windows に Nix を入れる

hook の登録の形:

- 案 X: exec form。`command` にバイナリの絶対パス、`args` に引数を書く
- 案 Y: shell form。今の Linux と同じく、Windows では Git Bash に絶対パスと引数の文字列を渡す

## 決定と理由

採用: 案 1、案 A、案 X。

- バイナリ: Windows では clone の各 crate を `cargo install --locked --force --path modules/home/parts/<crate>` で入れる
  （clone のルートに `Cargo.toml` は無い。crate の版は `0.1.0` のまま変えないので、`--force` で毎回入れ直す）。置き場は
  cargo の既定（`%USERPROFILE%\.cargo\bin`）。Linux は今のとおり Nix でビルドする
- 同期と settings.json の更新の先: ホームの下の `.claude`（今の `claude-sync.sh` と同じ）。`CLAUDE_CONFIG_DIR` には
  対応しない。hook のバイナリの置き場と Stop の hook の退避の先も `~/.claude` に固定していて、同期の先だけを変えると
  配備が 2 か所に分かれるため
- 設置のコマンド: 新しい crate に Rust で書き、サブコマンドで次を持つ。名前とサブコマンドの形は実装の PR で決める
  - 同期: `claude-sync.sh` を置き換える。規則（CLAUDE.md、skills、scripts はソースが正として中身で比べて上書き、
    commands は既存を残す、消さない、引数なしは既定の clone を `pull --ff-only` してからコピーする）は今のまま。既定の
    clone は、どちらの OS でもホームの下の `nix/claude-harness`（Windows では `%USERPROFILE%\nix\claude-harness`）。
    Linux の `claude-sync` もこれを呼ぶ。activation は今のとおり source（flake の `claudeCodeSource`）を引数で渡して
    呼び、clone を読まない
  - settings.json の更新: 静的な設定を Nix の attrset から、`.claude/` の外の JSON のファイルに移し、Nix は
    `builtins.fromJSON` で読む（`.claude/settings.json` はこのリポジトリのセッションのプロジェクトの設定で、
    `.claude/README.md` のとおり同期しない）。attrset の外で activation が足す `skipDangerousModePermissionPrompt` も
    その JSON に入れる。旧 bash hook のファイル（`~/.claude/hooks/`）の掃除は、Linux にしか無いので activation に残す。
    コマンドはそれを重ね、hook の登録を 1 件ずつに揃える（旧 bash hook の登録を外し、ほかの hook の登録を残す規則は
    今の jq の filter のまま）。既にある登録は、今の shell form（`command` の末尾がバイナリと引数）と exec form
    （`command` がバイナリで、`args` が引数）の両方で見分け、Windows の `\` 区切りのパスも見分ける。登録する hook と
    バイナリのパスは引数で受け、渡されたものだけを登録する。パスは、設置の時点でホームを展開した絶対パスで書く（exec
    form は `~` も `$HOME` も展開しない）。Linux の activation は `~/.claude/bin/` の `claude-hooks`（PreToolUse）と
    `claude-memory-backup`（Stop）を渡す。Windows は `cargo install` の置き場の、入れたバイナリだけを渡す（Stop の
    `claude-memory-backup` は jq と flock に頼る sh で、#101 の段階 8 で移すまで Windows には無い）。UserPromptSubmit
    の現在時刻（`date` の shell form）は、どちらの OS でも今の文字列のまま登録する（Windows では Git Bash の `date` が
    走る）
  - git hook の設置（Windows だけ）: hook のファイルを、Rust のバイナリを呼ぶだけの sh にして置き、`core.hooksPath`
    を向ける。中身の Rust は #101 の段階 4 で書くので、設置もその段階で足す。Linux は `git-hooks.nix` のまま
- Windows の手順: clone を `git pull` し、`cargo install` で設置のコマンドとほかの crate を入れ、clone のパスを渡して
  設置のコマンドを実行する（パスを渡すと pull しないので、ビルドした commit と同期する中身が揃う）。外部のスキル
  （flake input）は Windows では置かない
- Windows で使い始めるのは #101 の段階 3 から: Nix が包んで渡すもの（`claude-hooks` の `CLAUDE_HOOKS_JA_TERMS`、
  `package.nix` の `makeWrapper`）と PATH に頼る `gitleaks`、`~/.claude/bin/` に固定のパスで置く `ja-terms` を、Windows で
  どう用意するかは、ADR-0082 のとおり段階 3 で決める。崩れた語の検査は ADR-0085 で、Rust の ja-terms を `cargo install`
  で入れ、環境変数の無い Windows の claude-hooks は PATH で探すと決めた（#115）。
  それまでに PreToolUse hook を登録すると、`gitleaks` の無い Windows では機密情報の検査ができず、`block_secret_commit.rs`
  がすべての `git commit` を止める
- hook の登録: バイナリの hook は exec form にし、Linux も同じ形に揃える（例: `command` に
  `/home/<user>/.claude/bin/claude-hooks`、`args` に `["pre-tool-use"]`）

理由:

- 案 1 は、CI（ADR-0083）と同じ `Cargo.lock` と同じ target（MSVC）でビルドする。toolchain の版は固定していない
  （`rust-toolchain.toml` が無い）ので、CI の runner と手元の stable の版がずれることはある。CI が確かめるのは
  `windows.yml` の matrix に入れた crate だけで、2026-11-12 は `design-records` だけ。ほかの crate は、Windows で
  使う段階で matrix に足す。案 2 は、リリースの workflow、版の付け方、最初の 1 本（設置のコマンド自身）を入れる手順が要り、使うのが
  ユーザーの手元の機械だけの今は、その費用に見合わない。配る相手が増えたら見直す
- 案 A は、hook の登録と同期の規則が 1 つの実装になる。案 B は、今の jq の filter（旧 hook の除去、matcher の組の
  入れ替え）と rsync の規則を Rust に書き写し、2 つの実装が揃っているかを確かめ続ける費用がかかる。静的な設定の
  一覧（`claudeCodeStaticSettings` の 167 行、`a1a45cf5`）を Nix と Windows の 2 か所に持つことにもなる
- 案 C は、Git for Windows に jq と rsync が無いので、JSON の更新と同期をそれらに頼って書けない。ADR-0082 が Windows の
  依存から外した jq を、また入れることになる
- 案 D は、Nix がネイティブの Windows で動かない
- 案 X は、Bash ツールの呼び出しのたびに走る PreToolUse hook が、Git Bash を経ずに直接起動する。案 Y は、呼び出し
  ごとに Git Bash の起動を 1 つ足し、Windows のパス（`C:\Users\…`）をシェルの文字列に引用して書く必要がある。
  Linux も同じ形にするのは、settings.json の更新を OS で分けないため

### 帰結

- 良い点: Windows でも Linux でも、settings.json と `~/.claude` の中身が同じ規則で作られる。Windows の配備は jq と rsync
  に頼らない。Linux では、外部のスキルの配備（rsync）と Stop の hook（jq と flock）が、#101 の段階 8 まで残る
- 悪い点: Windows の機械に rustup と MSVC Build Tools（数 GB）が要り、更新のたびに手元でビルドする
- 悪い点: 外部のスキル（`grilling`、`grill-me`）と lsp-det のプラグインは Windows に置かれない。要るようになったら、
  設置のコマンドが取る形を決める
- 悪い点: Linux の rebuild が、設置のコマンドの crate のビルドとテスト（`package.nix` の check）を含むようになる
- 悪い点: 同期の規則は、今は `claude-sync.sh` を直せばその場で試せるが、Rust に移すとビルドが要る（ADR-0082 の帰結と
  同じ）。settings.json の PreToolUse の登録の形も、shell form から exec form に変わる
- git hook は #101 の段階 4 まで、Stop の hook（自動メモリの退避）は段階 8 まで、Windows に置かれない
- Git for Windows の既定（`core.autocrlf=true`）の clone では、同期する sh のスクリプトが CRLF になり、Git Bash で
  動かない。clone の直下の `.gitattributes`（`* text=auto eol=lf`）で、OS と利用者の `core.autocrlf` に依らず LF で
  取り出すようにした（PR #139。Windows の CI の `setup` が、同期したファイルに CR が無いことを確かめる）。design-records が
  CRLF の作業ツリー（利用者のほかのリポジトリ）を読めるかは #104 に残る
- 設置のコマンドの crate は `claude-setup`（`modules/home/parts/claude-setup`）。同期は `claude-setup sync` に移し、Linux の
  `claude-sync` と activation もそれを呼ぶ（2026-11-13）。settings.json の更新も `claude-setup settings` に移し、静的な設定は
  `modules/home/parts/claude-settings.json` に置く。activation は jq を使わず、このファイルのパスをコマンドに渡す（Nix で
  `builtins.fromJSON` で読む要は無くなった）。Linux の登録も exec form になった。Windows の手順は README の「Windows で
  使う」に書き（gitleaks は winget で入れる）、Windows の CI の `setup` のジョブがその手順を流して、登録した hook が main への
  `git commit` を止めることを確かめる（2026-11-13）。残りは git hook の設置（#101 の段階 4）。進みは #101 に書く

### 確認

- 設置のコマンドの crate を `windows.yml` の matrix に足し、同期と settings.json の更新の結合テストが Linux と
  Windows の両方で通る。テストの期待値は、今の jq の filter と `claude-sync.sh` が同じ入力から作る結果を、PreToolUse の
  登録だけ exec form に置き換えたものにする。shell form と exec form の既にある登録が、どちらも 1 件に揃うことも確かめる
- Linux で rebuild の前と後の `~/.claude/settings.json` を比べ、違いが hook の登録の形（exec form）だけである
- #101 の段階 3 の後、Windows の機械で設置のコマンドを実行してから、Claude Code の Bash ツールで main への `git commit` が PreToolUse hook に
  止められる

## 補足

- 2026-11-13: 同期（`modules/home/parts/claude-setup/src/sync.rs`）を claude-sync.sh の rsync から移す途中で、レビューで
  次の版を捨てた。どれも rsync -a と振る舞いが違った。
  - 中身が同じファイルを飛ばした。mode だけの変更（実行の bit）が ~/.claude に届かない
  - symlink を辿ってコピーした。親を指すリンクで止まらず、~/.claude の側のリンクを辿ると利用者の作業の場所に書く
  - 上書きを、消してからコピーした。読む側に消えたファイルを見せ、コピーに失敗すると前の版も無くなる
  - ファイルとディレクトリの入れ替わりで止まった（activation も止まる）
  - いちばん上の場所（skills など）のリンクを、中の項目と同じくリンクのままコピーした。rsync は末尾の `/` で辿る
  - 空の HOME を止めるテストを書いたが、`std::env::home_dir` は Unix で HOME が空なら passwd のホームを返し、テストが
    実際のホームに同期したので外した
