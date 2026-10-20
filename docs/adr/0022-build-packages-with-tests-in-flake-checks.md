---
status: accepted
date: 2026-10-11
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0022: テストのあるパッケージを flake の checks に入れ、nix flake check でサンドボックスのテストを実行する

## 背景

PR #305 のマージ後、rebuild で claude-hooks のビルドが失敗した。checkPhase（cargo test）の 12 件が
`git ["init", "-q", "-b", "main"] failed` で落ちた（`nix log` で確認）。原因は、Nix のサンドボックスの PATH に git が
無いこと。追加したテストは git で一時リポジトリを作るが、`package.nix` に `nativeCheckInputs = [ git ]` が無かった。
`crate::git::git` は spawn の失敗を `None` にするため、「git が無い」と「git が非ゼロで終わった」の区別が出ない。
`nativeCheckInputs` に git を足すと、同じ derivation のテスト 44 件が通った。この修正は別の PR（#309、main の f9ad102）で先に main に入り、
この PR の `package.nix` の変更は取り下げた。
測定は 2026-10-11、対象は main の e1e7e73（PR #305 のマージコミット）。失敗は `/nix/store/n5r31rf5kvfbywdnrfb1jrhx3f7rr0lm-claude-hooks-0.1.0.drv` の
`nix log`、修正後は `nix build .#packages.x86_64-linux.claude-hooks --no-link` のあとの `nix log`（`test result: ok. 44 passed`）で読める。

PR #305 の `run-checks.sh` は ALL_OK だった。`run-checks.sh` の nix の段は `nix flake check` だが、claude-hooks は
`packages` にしか無く、`nix flake check` は packages を評価するだけで build しない（checkPhase が走らない）。
普通のシェルの `cargo test` と `.claude/tests/hooks.sh` は通るので、サンドボックスだけで落ちる失敗は rebuild まで見つからなかった。

## 決定の要因

- rebuild の前に、PR の段階で気づく
- 既存の `run-checks.sh` の `nix flake check` をそのまま使い、新しい仕組みを足さない
- 変更のたびの待ち時間を増やさない

## 検討した案

- 案 A: `run-checks.sh` に、claude-hooks のソースが変わったときだけ `nix build .#claude-hooks` を実行する段を足す。
  共有スクリプトに claude-hooks 専用の分岐が入る。他のパッケージ（design-records など）にも同じ段が要る
- 案 B（採用）: `flake.nix` の `checks.x86_64-linux` にパッケージを入れる。`nix flake check` が build し、checkPhase が走る。
  壊した状態（`nativeCheckInputs` なし）で `nix flake check` が失敗することを確かめた
- 案 C: テストの helper で git の失敗を詳しく出す。診断は良くなるが、サンドボックスで落ちること自体は見つからない（別件）

## 決定と理由

案 B を採る。`nix flake check` は `checks` の出力を実現する。store にあるか substituter から取れる出力は build せず、
ローカルで build が必要なとき（ソースを変えた、GC した、別のマシン）にだけ checkPhase が走る。
テストを持つ `packages`（claude-hooks、commit-refs、design-records）をすべて入れる。テストの付いたパッケージを足すときは、同じ形で足す。

### 帰結

- `run-checks.sh` の nix の段が、claude-hooks のサンドボックスでのテストも実行する（ローカルで build するときは cargo の build 分だけ長くなる）

### 確認

- `package.nix` の `nativeCheckInputs` を外すと `nix flake check` が失敗し、戻すと通る
