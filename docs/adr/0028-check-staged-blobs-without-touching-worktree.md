---
status: accepted
date: 2026-10-12
requires: [ADR-0014, ADR-0015]
supersedes: []
superseded-by:
issues: ["#282"]
---

# ADR-0028: pre-commit の既定の検査は、ステージした版を書き出して見て、作業ツリーを書き換えない

## 背景

グローバルの pre-commit（`modules/home/parts/git.nix`）の既定の検査（nixfmt、ruff、markdownlint の自動修正、
rustfmt）は、作業ツリーのファイルを読んでいた。Markdown の自動修正は作業ツリーのファイルを直してから `git add` した。
`git add -p` などで一部だけステージしたファイルでは、ステージしていない編集がコミットに入り、ほかの検査はコミットに
入らない中身を見ていた（#282）。日本語の語（ADR-0014）と機密情報（ADR-0015）の検査は、すでにステージした版を見ていた。

## 検討した案

- 案 A: prek と同じく、検査の間だけステージしていない編集を差分（`git diff --binary`）として退け、作業ツリーを
  ステージした版にそろえ、検査の後で `git apply` で戻す。自動修正とぶつかったら、index を `git read-tree` で戻し、
  コミットを止める。実装してテストを通したが、ローカルのレビューで、戻せずに編集を失う場合が見つかり、捨てた
  （2026-10-12、一時のリポジトリで確かめた）
  - `diff=<driver>` の textconv があるファイルは、差分が変換後の中身になり、`git apply` できない。退けた差分にも
    元の中身が残らず、編集を取り戻せない
  - `diff.noprefix` を設定したリポジトリでは、差分に `a/` `b/` が付かず、`git apply` できない
  - `git add -N` のファイルは、そろえるときに空になり、戻すときに「既にある」で失敗し、`read-tree` が
    intent-to-add の印を消す
  - 検査のたびに、ステージしていない編集のあるファイルを 2 回書き換え、エディタや cargo の再ビルドを起こす
- 案 B（採用）: ステージした版（ブロブ）を、標準入力か一時のディレクトリで検査し、作業ツリーに触れない。Markdown の
  自動修正は書き出した版に当て、`git hash-object -w` と `git update-index --cacheinfo` でステージし直す
- 案 C: `git stash push --keep-index` で退ける。stash は worktree の間で共有され、並行するセッションの stash と
  混ざる。案 A と同じく作業ツリーを書き換える

## 決定と理由

案 B を採る。作業ツリーを書き換えないので、ステージしていない編集を失う経路が無く、差分の設定や index の状態
（textconv、`diff.noprefix`、intent-to-add）によらない。ADR-0014 と ADR-0015 の検査と同じく、コミットに入る中身を見る。

- Nix は `nixfmt --check -`、Python は `ruff --stdin-filename <パス> -` に標準入力で渡す（ruff はそのパスの場所から
  プロジェクトの設定を探す）
- Markdown は元のパスのまま一時のディレクトリに書き出し、リポジトリの直下の設定と `.markdownlintignore` を写して、
  そこで直して検査する。直した版をステージし直し、作業ツリーのファイルがステージした版と同じ（ステージしていない
  編集が無い）ときだけ、作業ツリーにも書く
- Rust は 1 つずつ標準入力で rustfmt に渡して整えた出力と比べる（ファイルで渡すと `mod` の先を読みにいき、標準
  入力の `--check` は差を見ない）。edition は最寄りの Cargo.toml のパッケージのもの（`cargo metadata` が workspace
  からの継承も解く）。クレートの全体を `cargo fmt --check` で見るのをやめ、ステージした `.rs` だけを見る

### 帰結

- 一部だけステージした Markdown の自動修正は、コミットには入るが、作業ツリーには入らない。作業ツリーのファイルは
  直す前の行のまま残り、次にその行をステージしたときにまた直る
- markdownlint の設定は、直下の決まった名前のファイル（ステージした版）だけを写す。設定の `extends` が指す別の
  ファイルは写らない
- rustfmt は標準入力で読むので、rustfmt.toml の `ignore` が効かない
- ruff と rustfmt は、ファイルごとに起動する（ファイルの数だけ起動の時間がかかる）
- 実装は `modules/home/parts/staged-checks.sh`（`git.nix` の `stagedChecks`）

### 確認

- `modules/home/parts/tests/pre-commit-partial-stage.sh`: 一部だけステージした Markdown、Nix、Python、Rust で、
  ステージした版を検査して直し、作業ツリーを書き換えないこと（中身と mtime）。`diff.noprefix`、textconv、
  `git add -N` のあるリポジトリでも、編集が残ること。改行の変換（`core.autocrlf`）があっても、全てステージした
  Markdown の作業ツリーを直すこと
- `modules/home/parts/tests/pre-commit-rust-format.sh`: クレートの edition（workspace からの継承を含む）と、
  クレートの外とパッケージの無い Cargo.toml の下の 2024
