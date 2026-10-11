---
status: accepted
date: 2026-10-11
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0022: テストのあるパッケージを flake の checks に入れ、nix flake check でサンドボックスのテストを走らせる

## 背景

PR #305 のマージ後、rebuild で claude-hooks のビルドが失敗した。checkPhase（cargo test）の 12 件が
`git ["init", "-q", "-b", "main"] failed` で落ちた（`nix log` で確認）。原因は、Nix のサンドボックスの PATH に git が
無いこと。追加したテストは git で一時リポジトリを作るが、`package.nix` に `nativeCheckInputs = [ git ]` が無かった。
`crate::git::git` は spawn の失敗を `None` にするため、「git が無い」と「git が非ゼロで終わった」の区別が出ない。
`nativeCheckInputs` に git を足すと、同じ derivation のテスト 44 件が通った。

PR #305 の `run-checks.sh` は ALL_OK だった。`run-checks.sh` の nix の段は `nix flake check` だが、claude-hooks は
`packages` にしか無く、`nix flake check` は packages を評価するだけで build しない（checkPhase が走らない）。
普通のシェルの `cargo test` と `.claude/tests/hooks.sh` は通るので、サンドボックスだけで落ちる失敗は rebuild まで見つからなかった。

## 決定の要因

- rebuild の前に、PR の段階で気づく
- 既存の `run-checks.sh` の `nix flake check` をそのまま使い、新しい仕組みを足さない
- 変更のたびの待ち時間を増やさない

## 検討した案

- 案 A: `run-checks.sh` に、claude-hooks のソースが変わったときだけ `nix build .#claude-hooks` を走らせる段を足す。
  共有スクリプトに claude-hooks 専用の分岐が入る。他のパッケージ（design-records など）にも同じ段が要る
- 案 B（採用）: `flake.nix` の `checks.x86_64-linux` にパッケージを入れる。`nix flake check` が build し、checkPhase が走る。
  壊した状態（`nativeCheckInputs` なし）で `nix flake check` が失敗することを確かめた
- 案 C: テストの helper で git の失敗を詳しく出す。診断は良くなるが、サンドボックスで落ちること自体は見つからない（別件）

## 決定と理由

案 B を採る。ソースが変わっていなければ store にあるので build は走らず、変わったときだけテストが走る。
いまは claude-hooks だけを入れる。ほかのパッケージにテストが付いたら同じ形で足す。

### 帰結

- `run-checks.sh` の nix の段が、claude-hooks のサンドボックスでのテストも走らせる（ソースを変えたときは cargo の build 分だけ長くなる）

### 確認

- `package.nix` の `nativeCheckInputs` を外すと `nix flake check` が失敗し、戻すと通る
