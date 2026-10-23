---
status: accepted
date: 2026-10-24
requires: [ADR-0036, ADR-0041]
supersedes: []
superseded-by:
issues: ["#461"]
---

# ADR-0057: 文書だけの差分で実行する Rust のテストをリポジトリが宣言する

## 背景

`run-checks.sh` は、ルートに `Cargo.toml` があれば差分の種類によらず `cargo fmt --check`、`cargo clippy --all-targets`、
全体の `cargo test` を実行する。xlc では文書（docs/、examples/*/verification.md）だけのブランチでも全クレートの clippy と
テストが走り、排他の Slurm のジョブ中は 1 コアに限られる（ADR-0041）ので 10 分を超え、前景のコマンドが 600 秒で切られた
（2026-10-23、#461）。

一方 xlc には文書を読むテストがある（`crates/docs/tests/consistency.rs` の文書どうしの整合と、検証の記録の表の
`--check`）。文書だけの差分で Rust の段を飛ばすと、これらを落とす。何を実行すれば文書の差分を検査できるかは
リポジトリごとに違い、nixfiles からは決められない。

## 検討した案

- 案 A（採用）: リポジトリが `scripts/run-rust-docs-tests.sh` を置いて宣言する。ルートに `Cargo.toml` とこのスクリプトが
  あり、差分が空でなくすべて `.md` なら、fmt、clippy、全体の `cargo test` の代わりにこのスクリプトだけを実行し、終了コードで
  判定する（実行権限が無ければ失敗）。Python のテストの実行方法をリポジトリが `scripts/run-python-tests.sh` で宣言する形
  （ADR-0036）と揃える
- 案 B: `.pre-commit-config.yaml` に `files: \.md$` の hook を書き、run-checks.sh がそれを読む。run-checks.sh は今、
  design-records の `id` の行を grep するだけで、hook の `entry` と `files` を読むには YAML の解釈が要る。prek に任せて
  実行させると、文書の差分のときだけ Rust の段を飛ばすという判断を run-checks.sh に残せない
- 案 C: `Cargo.toml` の `[workspace.metadata]` に `cargo test` の引数（`-p xlc-docs`）を書く。TOML を読むツールへの依存が
  増え、devShell を分けるなどの実行方法の違いを書けない
- 案 D: 文書だけの差分では Rust の段を飛ばす。xlc の文書を読むテストが走らなくなる
- 案 E: 「文書」とみなすパスもリポジトリが宣言する。今の要件（xlc の文書は `.md`）に要らない

「文書だけ」の判定は、`.md` の許可リストにする。`.rs`、`Cargo.toml`、`Cargo.lock` の拒否リストにすると、`clippy.toml`、
`rust-toolchain.toml`、`.cargo/config.toml`、テストが読むデータの変更で全体の検査を落とす。削除と rename の前後も数える
（`.rs` を消すだけの差分や、`.rs` から `.md` への rename を文書だけとみなさないため）。差分が空なら全体を実行する（今の挙動）。

## 決定と理由

案 A。宣言が無いリポジトリと、`.md` 以外を含む差分では挙動を変えないので、既存のリポジトリに影響しない。実行方法を
スクリプトに任せるので、`cargo test -p <crate>` のほか、devShell の切り替えや Python のツールの `--check` も書ける。
形が `scripts/run-python-tests.sh` と同じなので、置き場と失敗の扱い（実行権限、終了コード）を覚え直さなくてよい。

### 帰結

- 文書だけの差分では fmt と clippy を実行しない（`.md` は Rust のソースでなく、どちらの結果も変えない）
- 排他の Slurm のジョブ中は、スクリプトを `cargo` と同じく `taskset` の 1 コアで実行する
- スクリプトが文書を読むテストを漏らすと、文書だけのブランチでそのテストが走らない。宣言はリポジトリの責任で、
  漏れはマージ後の全体のチェック（`.rs` を含むブランチ）で見つかる
- `.md` 以外の文書（画像、`.txt`）を含む差分は全体を実行する

### 確認

- `.claude/tests/scripts/language-checks.sh` に、宣言があり差分が `.md` だけならスクリプトだけを実行すること（ステージ済みと
  `--base`）、スクリプトの失敗と実行権限の欠けで止まること、宣言が無いとき、`.rs` の変更と削除、`clippy.toml` を含むとき、
  差分が空のときは全体を実行することのテストを足した
