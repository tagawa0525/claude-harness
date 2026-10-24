---
status: accepted
date: 2026-10-24
requires: [ADR-0021, ADR-0032, ADR-0043]
supersedes: []
superseded-by:
amends: [ADR-0043]
issues: ["#478"]
---

# ADR-0059: ローカルレビューの記録の sha も、リベースだけの判定の比較元にする

## 背景

`pre-merge-check` の検査 8 は、リベースだけで hash が変わった head を、ADR-0021 の判定（`same_pr_diff`）で通す。
その比較元（レビュー済みの旧 head）は、最後の bot（Copilot）レビューの `commit_id` だけだった。ADR-0043 の
ローカルレビューの記録（`<!-- local-review sha=<sha> high=0 ... -->`）の sha は、今の head との完全一致にしか
使わず、ADR-0043 は「記録が無い head の扱い（リベースだけの ADR-0021 …）は変えない」と書いていた。

そのため、Copilot の 1 回目のレビューの後にローカルレビューで対応を重ねた PR は、main への rebase のたびに止まる。
比較が Copilot の見た対応前の head と新しい head の間になり、対応の差分が入るからである。
2026-10-23〜24 に PR #471 で 2 回、PR #475 で 1 回起き、そのたびに記録を付け直した（PR #475 では、記録した bead4d2 から
1426172 への変化が rebase だけであることを `git range-diff` で確かめてから記録した。issue #478）。

## 検討した案

- 案 A（採用）: 記録の sha も比較元にし、`same_pr_diff(base, 記録の sha, head)` が真なら通す。`decide-next.sh` の
  `LOCAL_REVIEWED` も同じ判定にする
- 案 B: 今のまま、rebase のたびに記録を付け直す。記録の手間に加え、差分が同じかを人（エージェント）が確かめる
  ことになり、ADR-0021 が機械に移した判断が戻る
- 比較元にする記録の選び方:
  - 「High 0 の記録のうち最後のもの」: sha A に High 0 の記録があり、その後に A を rebase しただけの A' に High 1 の
    記録が付いた場合、A' を rebase した head は A と同じ差分なので通ってしまう。同じ差分で見つかった High を前の記録で
    隠すことになり、ADR-0043 の「回し直して見つけた High を前の記録で隠さない」と食い違う
  - 「sha ごとに最後の記録で決め、High 0 の sha すべて」: 上と同じ理由で、後で High が見つかった差分と同じ差分の
    前の記録が残る。比べる回数も記録の数だけ増える
  - 「すべての記録のうち最後のもの。それが High 0 のときだけ」（採用）: 最後に下された判断だけを使う。迷ったら止める
    （ADR-0021 の決定の要因）側に倒れる

## 決定と理由

案 A を採る（ユーザーの決定、2026-10-24。issue #478）。これは ADR-0043 の「記録が無い head の扱い（…リベースだけの
ADR-0021 …）は変えない」と、マージの判定と `decide-next.sh` の「head に記録があれば」の部分を置き換える。ほかの部分は
変わらない。

- **比較元**: 書き込める人（OWNER / MEMBER / COLLABORATOR）のローカルレビューの記録のうち、最後の記録（sha を問わない）。
  それが High 0 のときだけ、その sha を比較元にする。High のある記録が最後なら、ローカルの記録からは比較元を取らない
- **マージの判定**（検査 8）: head に High 0 の記録がある（ADR-0043 のまま）か、上の比較元と head が ADR-0021 の判定で
  同じなら通す。bot レビューの `commit_id` を比較元にする判定（ADR-0021）も残す
- **判定の方法は ADR-0021 のまま**: base は GitHub が返す base ブランチの今の先頭、比べるのはそれぞれの merge-base からの
  PR の差分で、確かめられないときは同じとみなさない。比較元を増やしても、偽陽性の側の性質（中身が変わったのに通す
  ことはない）は同じである
- **`decide-next.sh`**: `LOCAL_REVIEWED` を、head の記録があるか、上の比較元と head が同じなら `yes` にする。判定は
  hook と同じ Rust の実装（`claude-hooks same-pr-diff <owner>/<repo> <base ブランチ> <旧 head> <新 head>`）を呼び、
  bash で書き直さない。比較元の選び方は `scripts/lib/local-review.sh` と `pre_merge_check.rs` の両方に書く
  （記録の読み取りが既に両方にあり、形を合わせる注記がある）
- **`decide-next.sh` の git**: 比べるには旧 head と新 head のコミットが要るので、cwd の clone で比べる（無ければ
  ADR-0021 と同じく origin から取る）。対象の PR は今も URL で決まる（ADR-0032）。cwd が PR のリポジトリの clone で
  なければ、確かめられないので `LOCAL_REVIEWED: no` になる（止める側）

### 帰結

- ローカルの記録の後に rebase しただけの PR は、記録を付け直さずにマージできる
- `decide-next.sh` が `claude-hooks` のバイナリ（`~/.claude/bin/claude-hooks`）に依存する。スキルのスクリプトから hook の
  バイナリを呼ぶのは初めてである
- `decide-next.sh` を PR の worktree の外で実行すると、rebase だけの head でも `LOCAL_REVIEW_NEEDED` になる。マージの
  hook は worktree で走るので通る。食い違いは止める側だけに出る
- main が PR と同じファイルの 3 行以内を変えたときに止まる外れ（ADR-0021 の帰結）は、記録を比較元にしても同じ

### 確認

- Rust のユニットテスト（`pre_merge_check.rs`）: 記録の読み取りが、最後の記録が High 0 ならその sha を、High ありなら
  何も返さないこと。`same_pr_diff` の既存のケースが通ったままであること
- `.claude/tests/hooks.sh` の pre-merge-check: 記録の sha から rebase だけした head は通す。記録の後に差分を変えた head、
  最後の記録が High ありの head は止める。`same-pr-diff` の入口が同じ判定を返すこと
- `.claude/tests/scripts/gh-pr-review.sh`: `decide-next.sh` が、記録の sha と head が同じ差分なら `LOCAL_REVIEWED: yes`、
  違えば `no` にし、最後の記録が High ありなら比べないこと
