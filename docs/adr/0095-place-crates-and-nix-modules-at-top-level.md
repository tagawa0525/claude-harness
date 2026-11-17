---
type: ADR
status: proposed
date: 2026-11-17
requires: [ADR-0082, ADR-0084, ADR-0094]
supersedes: []
superseded-by:
issues: ["#283"]
---

# ADR-0095: Rust の crate は crates/ に、Nix のモジュールは nix/ に置き、modules/home/parts/ をやめる

## 背景

このリポジトリは 2026-11-12 に nixfiles から切り出し、履歴はハーネスのパスだけを残して引き継いだ。hook と CLI、
home-manager のモジュールは、nixfiles のときのパス `modules/home/parts/` にそのまま残っている。

- `modules/` の下は `home/` だけ、`home/` の下は `parts/` だけで、3 段の階層に分岐が無い。nixfiles では `modules/nixos/`、
  `modules/home/users/`、`modules/home/scripts/` などと並んでいたが、このリポジトリにはその並びが無い
- `modules/home/parts/` の追跡しているファイル 268 のうち、Rust の crate が 250（claude-hooks、claude-setup、commit-refs、
  design-records、fix-markdown-lint、history-rewrite、ja-terms の 7 つ）、Nix でビルドした hook を流す sh のテストが 14、
  Nix のモジュールとその部品が 3（`claude-code.nix`、`git-hooks.nix`、`lib/git-check-config.nix`）、`claude-setup settings` に
  渡す静的な設定が 1（`claude-settings.json`）。リポジトリ全体は 454（2026-11-17、a3155d81）
- ハーネスのツールは Rust に寄せ、ネイティブの Windows では Nix を使わずに `cargo install --path modules/home/parts/<crate>` で
  入れる（ADR-0082、ADR-0084）。Nix を使わない利用者が、home-manager のモジュールの階層の名前を打つ

リポジトリに入った人は、名前から中身（大半が Rust）を読み取れない。

## 決定の要因

- ディレクトリの名前から、中身の種類（Rust の crate、Nix のモジュール）が分かること
- Nix を使わない設置（Windows）の手順に、Nix の階層の名前が出ないこと
- 利用する側（nixfiles）が読むもの（flake の出力、`claude/`、`config/`）が変わらないこと
- 挙動を変えない移動にとどめ、ビルドの作り直しの範囲を変える決定を混ぜないこと

## 検討した案

### 案 A: crate を `crates/` に、Nix のモジュールとそのテストを `nix/` に置く（採用）

```text
crates/<crate>/        Cargo.toml、src/、tests/、package.nix
nix/claude-code.nix    home-manager のモジュール
nix/git-hooks.nix
nix/lib/               package.nix が共有する部品（git-check-config.nix）
nix/tests/             Nix でビルドした hook を流す sh のテスト
claude-settings.json   claude-setup settings --static に渡す静的な設定
```

- 長所: Rust のリポジトリでよく使う名前（`crates/`）で、中身が分かる。Windows の手順は `cargo install --path crates/<crate>` に
  なる
- 長所: `package.nix` は crate の隣に残り、crate どうしの相対の参照（`../ja-terms/package.nix`、`lib.fileset` の `../.`）は
  変わらない
- 短所: パスを書いているコード、テスト、文書を直す（ADR を除いて 180 行。2026-11-17、a3155d81）。確定した ADR の凍結する
  部分には `modules/home/parts/` が残る
- 短所: `package.nix` から `nix/lib/` への参照が `../../nix/lib/` になり、crate の外の階層をまたぐ

### 案 B: 今のまま

- 長所: 変更が無い
- 短所: 要因の 1 つ目と 2 つ目を満たさない

### 案 C: crate をリポジトリの先頭に並べる（`claude-hooks/`、`ja-terms/` …）

- 長所: パスが最も短い
- 短所: 先頭に 7 つのディレクトリが増え、`claude/`（配る元）と `claude-hooks/`、`claude-setup/`（crate）が同じ段に並んで
  紛らわしい

### 案 D: 案 A に加えて、`crates/` を Cargo の workspace にする

- 長所: `cargo test --workspace` を 1 回で流せ、`target/` を共有する
- 短所: `Cargo.lock` が 1 つになり、どれかの crate の依存を更新すると、Nix では lock を読む crate がすべて作り直しになる。
  今の `package.nix` は `lib.fileset` で crate ごとにソースを絞っている。移動（挙動を変えない）と、作り直しの範囲を変える
  決定が 1 つの変更に混ざる

### 案 E: 案 A で、`package.nix` も `nix/` に集める

- 長所: Nix のファイルが 1 か所に集まる
- 短所: `package.nix` は crate のソースと `Cargo.lock` を相対パスで指すので、すべての参照が階層をまたぐ。crate と
  ビルドの定義を隣に置く nixpkgs の慣習からも離れる

## 決定と理由

案 A。

- crate は `crates/<crate>/` に、`package.nix` を隣に置いたまま移す
- home-manager のモジュール（`claude-code.nix`、`git-hooks.nix`）と `lib/` は `nix/` に移す。`flake.nix` はリポジトリの
  先頭に残す
- Nix でビルドした hook を流す sh のテスト（`modules/home/parts/tests/`）は `nix/tests/` に移す。`claude/tests/`（cargo で
  ビルドしたバイナリで流し、Linux の CI が流す）とは、Nix が要るかで分かれる
- `claude-settings.json` はリポジトリの先頭に置く。Nix の activation と Windows の設置の両方が読むので `nix/` には
  入れない。`claude/` は `~/.claude` に、`config/` は `~/.config` に配る元なので、どちらに入れても配る対象に見える。
  `crates/claude-setup/` に入れると、許可の一覧を変えるたびに crate のソースが変わる
- `post-merge-cleanup` がハーネスのリポジトリを見分ける目印は `crates/claude-setup/Cargo.toml` に、lock の更新と rebuild を
  知らせる変更は `crates`、`nix`、`claude-settings.json`、`flake.nix`、`flake.lock` にする。古いパスの目印は残さない
  （ADR-0094 と同じく、古い名前を探す分岐を残すと名前が 2 つある状態が続く）。ハーネスを分ける前の nixfiles の目印
  （`modules/home/scripts/claude-sync.sh`）は、別のリポジトリのパスなので変えない
- 案 C は、配る元と crate が同じ段で紛らわしい。案 E は、相対の参照が増える
- 案 D は、作り直しの範囲を測ってから別に決める（#283）

### 帰結

- 良い点: 先頭の `ls` で、Rust の crate、Nix のモジュール、配る元、文書が分かれて見える
- 悪い点: 確定した ADR の凍結する部分と、マージの前のコミットの本文には `modules/home/parts/` が残る。この ADR より前の
  記録の `modules/home/parts/<crate>` は `crates/<crate>`、`modules/home/parts/tests` は `nix/tests`、
  `modules/home/parts/claude-settings.json` は `claude-settings.json`、それ以外の `modules/home/parts/<パス>` は `nix/<パス>` と
  読む（`modules/home/scripts/` は nixfiles のパスで、変わらない）
- 配ってある版の `claude-hooks` の `post-merge-cleanup` は、古い目印（`modules/home/parts/claude-setup/Cargo.toml`）を探す。
  この変更を取り込む pull では、差の起点に目印があるので知らせが出る。その後、取り込む側（nixfiles）で lock を進めて
  rebuild するまで（Windows では `cargo install` し直すまで）、ハーネスの pull の後の `claude-sync` と lock の更新の知らせは
  出ない
- Windows のホストは、設置の手順のパスが変わる（`docs/windows.md`）。入れてあるバイナリは、入れ直すまでそのまま動く
- workspace にするかは決めていない（#283）

### 確認

- `nix flake check`: 7 つのパッケージのビルドとテスト、テスト用の構成の activation（モジュールの評価）
- `.github/workflows/linux.yml`、`.github/workflows/windows.yml`: `crates/<crate>` での `cargo test`、Windows の設置の手順
- `claude/tests/scripts/git-workflow.sh`: `crates/claude-setup/Cargo.toml` を持つリポジトリで、`crates/` が変わる pull の
  後に lock の更新と rebuild を知らせる
- `nix/tests/` の sh のテスト: Nix でビルドした hook での判定
