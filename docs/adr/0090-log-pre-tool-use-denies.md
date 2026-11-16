---
status: accepted
date: 2026-11-16
requires: [ADR-0033]
supersedes: []
superseded-by:
issues: ["#21"]
---

# ADR-0090: PreToolUse の deny を、状態ディレクトリの deny.jsonl に 1 件 1 行で追記する

## 背景

CLAUDE.md の文章で守らせている規則のうち、どれを hook に移すか、どの hook の規則が要らないかを決める材料がない。
claude-hooks は deny を Claude に返すだけで、どのルールが、いつ、どのリポジトリで、何回止めたかが残らない（#21）。
ルール名は、deny の理由の先頭に `[<name>]` と空白 として付く（`output::label_denies`）。

決めること: 記録する項目（コマンドは秘密を含みうる）、置き場、記録の失敗の扱い、Context と判定できなかったときと
エスケープの扱い。

## 検討した案

置き場:

- 案 1: `$XDG_STATE_HOME/claude-harness/deny.jsonl`（未設定なら `~/.local/state/claude-harness/`、Windows は
  `%LOCALAPPDATA%\claude-harness`）。#21 は `claude-hooks/` を候補にしたが、ハーネス全体の状態の置き場として
  `claude-harness/` にする
- 案 2: `~/.claude/` の下。Claude Code の設定と同期の対象に混ざり、秘密を含みうる記録が共有に流れうる

コマンドの記録:

- 案 A: 全文を残す。秘密が全文でファイルに残る
- 案 B: 先頭 200 文字に切る。長い heredoc などの本文は残らず、秘密の多くは切れるが、先頭にあれば残る
- 案 C: 残さない。どの操作が止まったかが分からず、規則の見直しに使えない

記録の失敗: systemMessage で知らせる案（#21 の本文）は、ログが補助であり、毎回の画面の通知が deny の理由を
埋めるので採らない。

## 決定と理由

PreToolUse でルールが Deny を出すたびに、次の項目の JSON を 1 行で `deny.jsonl` に追記する（案 1、案 B）:
`time`（UTC の ISO 8601）、`rule`、`cwd`、`session_id`（入力にあれば）、`command`（先頭 200 文字）、`reason`（ルール名の
接頭辞を除く先頭 200 文字）。

- ファイルは 0600 で作る（Unix）。コマンドに秘密が含まれうるので、先頭 200 文字に切ったうえで、ほかの利用者に読ませない
- 書き込みの失敗は無視し、判定も出力も変えない（ログは補助。回復を要する状態でもない）
- 環境変数 `CLAUDE_HARNESS_DENY_LOG=0` で記録しない
- 記録は Deny だけにする。Context（warn-large-commit）は止めていない。判定できなかった理由は規則の見直しの材料でない。
  `ALLOW_*` のエスケープで通したときは Deny の理由が付かず、記録しない。要るなら #21 に戻して足す
- ローテーションと集計のツールは作らない。件数が増えて困ってから決める

### 帰結

- 良い点: 規則ごと・リポジトリごとの deny の回数を、実際の記録から数えられる
- 悪い点: コマンドの先頭が平文でディスクに残る。0600 と 200 文字の切り詰めで抑えるが、無くならない
- 良い点: 追記は std の `File::lock`（プロセス間のロック）の間に書くので、並行する hook の行が混ざらない。既存のファイルの権限も 0600 に締める
- 悪い点: ファイルは大きくなり続ける
- 悪い点: テストが deny を出すたびに利用者の記録が汚れる。claude-hooks の crate の `.cargo/config.toml` で
  `CLAUDE_HARNESS_DENY_LOG=0` を cargo の実行に渡して防ぐ（記録を確かめるテストだけが `XDG_STATE_HOME` を向ける）

### 確認

- `tests/deny_log.rs` が、記録の項目、200 文字の切り詰め、0600、`CLAUDE_HARNESS_DENY_LOG=0`、書き込めないときも判定が変わらないこと、既存のファイルの権限を 0600 に締めること、並行する hook の行が混ざらないことを確かめる
