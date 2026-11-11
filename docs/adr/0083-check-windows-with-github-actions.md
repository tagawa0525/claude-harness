---
status: accepted
date: 2026-11-12
requires: [ADR-0082]
supersedes: []
superseded-by:
issues: ["#101", "#104"]
---

# ADR-0083: Windows での動きを GitHub Actions の windows-latest の cargo test で確かめる

## 背景

ADR-0082 は、ハーネスの hook とスクリプトを Rust に寄せ、ネイティブの Windows で Linux と同じ判定をすることにした。
Linux のテストは `nix flake check` が流す（ADR-0022）が、Windows で確かめる場所が無い。このリポジトリには
`.github/workflows/` がまだ無く、リポジトリは公開なので GitHub Actions の Windows のランナーは無料で使える
（2026-11-12、`gh repo view --json visibility` が `PUBLIC`）。

`windows-latest` で 3 つの crate の `cargo test --locked` を流すと、どれもコンパイルの段階で落ちた（2026-11-12、
試しの run 43071331951）。原因は `std::os::unix` の使用で、内訳は次のとおり。

- `claude-hooks`: 本体の 1 か所（`check_merge_body_ja_terms.rs` の、hook の実行ビットを `mode()` で見る判定）と、
  テストの約 20 か所（偽の検査を sh のスクリプトで作り `from_mode(0o755)` を付ける、symlink）
- `commit-refs`: 本体の 4 ファイル（`OsStrExt` で git の出力のバイト列をパスにする）と、テストの約 16 か所
- `design-records`: テストの 8 か所だけ（偽の git、symlink、mode 000）

テストの多くは、外のコマンド（`git`、`commit-msg-check` など）の偽物を sh のスクリプトで作って PATH の先頭に置き、
失敗の分岐を起こす。Windows の Rust の `Command` は PATH から `.exe` しか探さないので、sh の偽物を呼べない。

## 決定の要因

- 手元に Windows が無くても、PR ごとに Windows で確かめられること
- main の CI を、Windows で通らない crate のために赤くしないこと
- 偽物を作り直す費用が、確かめたいもの（OS で変わる部分）に見合うこと

## 検討した案

確かめる場所:

- 案 A: 手元の Windows の PC で、移す PR ごとに手で `cargo test` を流す
- 案 B: GitHub Actions の `windows-latest` で、PR と main への push ごとに `cargo test --locked` を流す

偽物に頼るテストの扱い:

- 案 1: `#[cfg(unix)]` で Unix だけで流し、Windows では偽物の要らないテストを流す
- 案 2: 偽物を Rust のテスト用のバイナリ（`.exe`）に作り直し、両方の OS で全部のテストを流す
- 案 3: 本体が環境変数で外のコマンドのパスを受け、テストはそこに偽物を渡す

## 決定と理由

採用: 案 B と案 1（2026-11-12 のユーザーの決定）。

- `.github/workflows/windows.yml` が、crate ごとの matrix で `cargo test --locked` を流す。matrix に入れるのは
  Windows で通る crate だけで、残りの crate は Windows に移す PR（#101 の段階）で通してから足す
- 偽物、symlink、mode に頼るテストは `#[cfg(unix)]` にし、理由をその場のコメントに書く。テストの一部だけが
  Unix に頼るときは、その部分だけを `#[cfg(unix)]` にして、残りを Windows でも流す

理由:

- 案 A は、確かめるかが人の手順に頼り、移した後の変更で Windows を壊しても気付けない
- 偽物が起こすのは git の失敗などのエラーの分岐で、分岐の中身は OS によらないので Linux で確かめれば足りる。
  Windows で確かめたいのは、パス、改行、文字コード、プロセスの起動のような OS で変わる部分で、これは偽物の
  要らない正常の動きのテストで確かめられる
- 案 2 は網羅が最大だが、sh の `case` で書いた偽物を crate ごとに書き直す費用が、上の理由で得るものに見合わない
- 案 3 は、本体にテストのためだけの入口が増え、偽物を Windows で動かす問題も残る

### 帰結

- 良い点: PR ごとに Windows で確かめられ、Windows を壊す変更が PR の段階で見つかる
- 悪い点: エラーの分岐は Windows では確かめない。OS で変わる処理がエラーの分岐の中にあれば見落とす
- `design-records` を Windows で流すと、Git for Windows の既定の `core.autocrlf=true` で作業ツリーが CRLF になり、
  LF を期待したテストが 4 件落ちた（run 43072103532）。テストのリポジトリで改行の変換を止めて通し、CRLF の
  作業ツリーでの動きを確かめることは #104 に残した
- 実装はまだ決定に追いついていない。matrix は `design-records` だけで、`claude-hooks` と `commit-refs` は #101 の
  段階で足す

### 確認

- PR の checks に `windows` のワークフローの結果が出て、matrix の crate のテストが通る
- `#[cfg(unix)]` にしたテストには、その場に理由のコメントがある
