---
status: accepted
date: 2026-10-12
requires: []
supersedes: []
superseded-by:
issues: ["#329"]
---

# ADR-0033: claude-hooks は判定できない理由を systemMessage で利用者に見せる

## 背景

claude-hooks は、git を起動できない（PATH に無い、ディレクトリに入れない）ときの理由を標準エラーに出し（#320）、
ルールの panic を拾って何も出さずに終わる。どちらも終了コードは 0 なので、Claude Code は標準エラーを利用者にも
Claude にも見せない（debug log だけに残る）。判定できなかったことに、テストのほかは誰も気付けない。

公式ドキュメント（<https://code.claude.com/docs/en/hooks>、2026-10-12 に読んだ）によると、終了コードを 0 以外にすると
「hook error」として画面に出るが、ほかのルールの deny と同時に起きたとき、stdout に JSON が無ければ deny が消えて
ツールが走る。最上位の `systemMessage` は「Warning message shown to the user」で、判定と同時に返せる。

実機（2026-10-12、Claude Code v2.1.296、ユーザーがリモートから ssh 越しのターミナルで見た。#329 のコメント）で、
PreToolUse の hook の出力を 2 通り確かめた。

- deny と `systemMessage` を同じ JSON で返す: 画面に `PreToolUse:Bash says: <systemMessage>` が出て、deny も効いた
- `systemMessage` だけを返す（判定なし）: 同じ形で画面に出て、ツールは走った。文面を書くよう頼んでも、モデルは
  「警告は出ていない」と答えた（モデルには渡らない）

## 検討した案

- 案 A: 判定できない理由は、判定の有無によらず最上位の `systemMessage` に入れ、終了コードは 0 のままにする
- 案 B: 判定があるときは `systemMessage`、判定が無いときは終了コード 1 で stdout を空にし、理由を標準エラーに出す
  （画面に「hook error」として出る）
- 案 C: 今のまま標準エラーに出す

## 決定と理由

採用: 「案 A」。

- 判定（deny、context）を失わずに、利用者の画面に出る。実機で、判定のあるときもないときも出ることを確かめた
- 出し方が 1 つで済む。案 B は判定の有無で出し方が分かれ、テストも終了コードと JSON の両方を見ることになる
- モデルには渡らないが、渡す必要はない。判定できない理由は環境（git の PATH、ディレクトリの権限）や claude-hooks の
  不具合で、モデルが直すものではなく、利用者が見て直すもの
- gh を起動できないときも、git と同じく理由を出す（gh を使うルールも判定しないまま通すため）
- 拾った panic も「判定できない」なので、どのルールが止まったかと panic の文を `systemMessage` に出す。panic は
  ルールごとに拾い、ほかのルールの判定（deny）を残す。panic の文は今までどおり標準エラーにも出る（テストが
  panic を見分けるため）
- 同じ理由（同じディレクトリでの同じ失敗）は 1 回だけ出す（1 回の判定で git や gh を何度も呼ぶため）。ディレクトリが
  違えば理由も違う（入れないのはそのディレクトリだけかもしれない）ので、それぞれ出す
- hook の入力を読めないときも、判定していないことを出す

採らなかった案:

- 案 B: 判定の無いときだけ別の出し方になる。`systemMessage` で足りることを実機で確かめた
- 案 C: 誰にも見えない（背景）

### 帰結

- 良い点: 判定できなかったことが利用者の画面に出る。deny と同時でも失われない
- 悪い点: モデルは判定できなかったことを知らないまま進む。ルールが判定しなかったぶん、ゲートは素通りになる（今と同じ）
- テストの補助（`.claude/tests/lib.sh` の `run_hook`、`hooks-diff.sh` の `decide()`）は、標準エラーに加えて
  `systemMessage` の有無でも「判定できなかった」を見分け、空の出力を allow と読まずに止める
- 見直す条件: Claude Code の `systemMessage` の表示が変わったとき

### 確認

- `modules/home/parts/claude-hooks/tests/git_spawn.rs` で、git と gh を起動できない理由が `systemMessage` に 1 回だけ出て、
  標準エラーには出ないこと。deny と同時でも判定が残ること
- panic を拾う経路は、ルールを panic させる手段がテストに無いので、テストで確かめていない
