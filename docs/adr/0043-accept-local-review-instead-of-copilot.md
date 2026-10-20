---
status: accepted
date: 2026-10-21
requires: [ADR-0004, ADR-0021, ADR-0023]
supersedes: [ADR-0034]
superseded-by:
issues: ["#404"]
---

# ADR-0043: ローカルレビューの記録があれば、Copilot のレビューの有無を問わずマージを通す

## 背景

ADR-0034 は、Copilot への再レビューを「直前のレビューに High が残るとき」と「周回を数え直したとき」に限り、
それ以外は head のローカルレビューの記録（`record-local-review.sh`）で終えると決めた。マージの判定
（`pre-merge-check` の検査 8）は、その記録を通す条件に Copilot のレビューの状態を残した。直前の Copilot の
レビューに High が残らない（本文が v2 の形で分かる）こと、その後に数え直していないこと、最後の push で
Copilot のチェックが失敗していないことである。

2026-10-21、PR #400 で Copilot が利用枠の上限でレビューできなかった（`decide-next.sh` の HEADLINE が quota limit、
`VERDICT: REVIEW_FAILED`）。head のローカルレビューを 2 周記録していたが、検査 8 が止めた（issue #404）。
Copilot のレビューが来ない、または来ても本文が読めない PR は、ローカルレビューの記録があってもマージできない。

同じ日に、ユーザーが「ローカルレビュー（/code-review）を GH Actions（Copilot）のレビューの代わりにしてよい」と決めた。

## 検討した案

- 案 A: 従来どおり（ADR-0034）。Copilot のレビューが来ない PR は、`ALLOW_UNREVIEWED_HEAD` で外すしかない。
  利用枠の上限のたびに ADR-0023 の逃げ道を使うことになり、逃げ道が常用される
- 案 B: Copilot が利用枠の上限でレビューできなかったときだけ、記録で通す。上限は `decide-next.sh` の
  HEADLINE で機械的に判定できるが、本文の文言（quota limit）に依存する。ほかの失敗（内部エラー、レビューが来ない）
  で詰まる状況は残り、Copilot を頼る前提が残る。ユーザーの決定は枠切れに限っていない
- 案 C（採用）: head のローカルレビューの記録（High なし）があれば、Copilot のレビューの有無・到着・内容を問わず通す

## 決定と理由

案 C にする（ユーザーの決定、2026-10-21）。ADR-0034 を置き換え、まだ有効な部分をここに含める。

- **マージの判定**（検査 8）: head に、書き込める人（OWNER / MEMBER / COLLABORATOR）の `<!-- local-review sha=<head> high=0 ... -->`
  の記録があれば通す。同じ head に記録が複数あれば最後の記録で決め、High のある記録は数えない（High を直せば head が
  変わるので、その head には直していない High が残る）。Copilot のレビューが 1 件も無い、チェックが失敗している、
  本文が読めない、High が残る、周回を数え直した、のどれでも通す。記録が無い head の扱い（文書だけの PR の
  ADR-0004、リベースだけの ADR-0021、`ALLOW_UNREVIEWED_HEAD` の ADR-0023）は変えない
- **Copilot の指摘への対応**: Copilot のレビューが来ていれば、その未解決スレッドへの対応は検査 6 が従来どおり要る。
  本文だけに出る Previously missed の指摘はスレッドが無く、hook は見ない。`get-latest-review.sh` が本文から取り出して
  見せるので、`decide-next.sh` の出力を読むモデルが台帳に処置を書く
- **`decide-next.sh`**: head の記録があり未解決スレッドが無ければ `STOP_LOCAL_REVIEWED`（Copilot の未着、失敗、
  High の残りを問わない。head を Copilot がレビュー済みなら従来どおり本文に応じた `STOP_*` で、どれもマージへ進む）。未解決スレッドがあれば `ACT`。記録が無ければ従来どおり（`LOCAL_REVIEW_NEEDED`、
  `REVIEW_FAILED`、`WAITING`）。再レビューの要求を促す `REREVIEW_NEEDED` と、周回の上限の `STOP_LIMIT`、
  `COPILOT_NEEDED`、`MAX_ROUNDS` は外す（記録があれば終わるので、Copilot の周回を数えて止める必要が無い。
  ADR-0023 の「`MAX_ROUNDS` は残す」は、この決定で置き換わる）。`ROUND` は情報として残す
- **Copilot の待ちと再レビューの要求は任意**: `gh-wait-review.sh` で待つのも、`request-rereview.sh` で再レビューを
  要求するのも、Copilot の指摘を受けたいときだけ。`request-rereview.sh` は、head の記録が無ければ要求しない
  （ADR-0034 のまま）。記録は `record-local-review.sh` が付け、重大度は Copilot の High / Medium / Low に揃える
- **受け入れる弱点**: ローカルレビューの記録はエージェントの自己申告で、回したかを機械では確かめられない
  （ADR-0034 と同じ）。加えて、Copilot の High が残っていても通る。記録の件数と、後から来た Copilot の指摘を同じ PR の
  中で比べて、効果を測れる

## 帰結

- Copilot の利用枠が尽きても、レビューが来なくても、記録があればマージできる。Copilot の消費は、受けたいと
  選んだ PR に限られる
- `pre_merge_check.rs` の Copilot の本文の読み取り（`remaining_high`）は判定に使わなくなったので外した。
  `get-latest-review.sh` の `REMAINING_HIGH` は、台帳のために残る
- 検査 8 の拒否の文言は、記録が無い head に対して従来の次の一手（`decide-next.sh`）を案内する

## 確認

- `.claude/tests/hooks.sh` の pre-merge-check: 記録があれば Copilot のレビューが無くても、チェックが失敗していても、
  数え直しの後でも通すこと。記録が無い、前の head の記録、SHA の前方一致、High のある記録、後の記録の High、
  コメントを取得できないときは止めること。記録は書き込める人のコメントだけを読むこと
- `.claude/tests/scripts/gh-pr-review.sh`: `decide-next.sh` が、記録があれば未着、失敗、High の残りを問わず
  `STOP_LOCAL_REVIEWED` にし、未解決スレッドがあれば `ACT` にすること。記録が無ければ従来どおり。
  `--max-rounds` を受けないこと
