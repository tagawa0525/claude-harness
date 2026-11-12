---
status: proposed
date: 2026-11-12
requires: [ADR-0082, ADR-0083]
supersedes: []
superseded-by:
issues: ["#101"]
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
  `cargo test --locked` は、GitHub Actions の `windows-latest` で同じ toolchain を使ってビルドしている

## 決定の要因

- Windows と Linux で、hook の登録と同期の規則を 2 つの言語に書き分けないこと（ADR-0082 の要因と同じ）
- Linux の今の配備（rebuild で入れる、`claude-sync` で入れる）の振る舞いを変えないこと
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

- バイナリ: Windows では clone の各 crate を `cargo install --locked --path` で入れる。置き場は cargo の既定
  （`%USERPROFILE%\.cargo\bin`）。Linux は今のとおり Nix でビルドする
- 設置のコマンド: 新しい crate に Rust で書き、サブコマンドで次を持つ。名前とサブコマンドの形は実装の PR で決める
  - 同期: `claude-sync.sh` を置き換える。規則（CLAUDE.md、skills、scripts はソースが正として中身で比べて上書き、
    commands は既存を残す、消さない、引数なしは既定の clone を `pull --ff-only` してからコピーする）は今のまま。Linux の
    `claude-sync` と activation もこれを呼ぶ
  - settings.json の更新: 静的な設定を Nix の attrset から JSON のファイルに移し、Nix は `builtins.fromJSON` で読む。
    コマンドはそれを重ね、hook の登録を揃える（今の jq の filter と同じ規則）。登録するバイナリのパスは引数で受け、
    Linux の activation は今の `~/.claude/bin/` を、Windows は `cargo install` の置き場を渡す
  - git hook の設置（Windows だけ）: hook のファイルを、Rust のバイナリを呼ぶだけの sh にして置き、`core.hooksPath`
    を向ける。中身の Rust は #101 の段階 4 で書くので、設置もその段階で足す。Linux は `git-hooks.nix` のまま
- Windows の手順: clone を `git pull` し、`cargo install --locked --path` で設置のコマンドとほかの crate を入れ、
  設置のコマンドを実行する。外部のスキル（flake input）は Windows では置かない
- hook の登録: exec form にし、Linux も同じ形に揃える（`command` に `~/.claude/bin/claude-hooks` の絶対パス、`args` に
  `["pre-tool-use"]`）

理由:

- 案 1 は、CI（ADR-0083）と同じ toolchain と `Cargo.lock` でビルドするので、CI で通ったものが手元でも同じにビルド
  できる。案 2 は、リリースの workflow、版の付け方、最初の 1 本（設置のコマンド自身）を入れる手順が要り、使うのが
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

- 良い点: Windows でも Linux でも、settings.json と `~/.claude` の中身が同じ規則で作られる。jq と rsync が配備の依存から
  外れる
- 悪い点: Windows の機械に rustup と MSVC Build Tools（数 GB）が要り、更新のたびに手元でビルドする
- 悪い点: 外部のスキル（`grilling`、`grill-me`）と lsp-det のプラグインは Windows に置かれない。要るようになったら、
  設置のコマンドが取る形を決める
- 悪い点: Linux の activation が、設置のコマンドのビルドを待つようになる
- git hook は、#101 の段階 4 までは Windows に置かれない
- 実装はまだ決定に追いついていない。進みは #101 に書く

### 確認

- 設置のコマンドの crate を `windows.yml` の matrix に足し、同期と settings.json の更新の結合テストが Linux と
  Windows の両方で通る。テストは、今の jq の filter と `claude-sync.sh` が同じ入力から作る結果を期待値に使う
- Linux で rebuild の前と後の `~/.claude/settings.json` を比べ、違いが hook の登録の形（exec form）だけである
- #101 の段階 3 の後、Windows の機械で設置のコマンドを実行してから、Claude Code の Bash ツールで main への `git commit` が PreToolUse hook に
  止められる
