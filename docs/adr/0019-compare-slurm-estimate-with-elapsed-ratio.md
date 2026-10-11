---
status: accepted
date: 2026-10-11
requires: [ADR-0007, ADR-0017]
supersedes: []
superseded-by:
issues: []
---

# ADR-0019: Slurm の見積もりと実績の比を、COMPLETED で実際に走ったジョブだけで求める

## 背景

ADR-0017 で、終わったジョブを `/var/log/slurm/job_completions`（jobcomp/filetxt）に残すことにした。見積もり
（`--time`）が妥当だったかを読むには、この記録から見積もりと実績の比を求める規則が要る。記録には次の癖がある
（2026-10-11 の実ファイルで確認）。

- Elapsed の欄が無い。`EndTime - StartTime` で求める
- 開始前に `CANCELLED` で消えたジョブは `NodeCnt=0` で、`StartTime` と `EndTime` が同じになる
- `FAILED` や `TIMEOUT` のジョブは走った時間があるが、最後まで走っていない
- 見積もりは上限ではない（ADR-0007、`OverTimeLimit=UNLIMITED`）。実績が見積もりを超えるジョブがありうる

## 決定の要因

- 見積もりの妥当性の判断に、最後まで走った実績だけを使う（途中で終わった短い実績が比を下げない）
- 同じ入力から同じ表が出る（モデルの判断を入れない）
- 外部の依存を足さない

## 検討した案

- 案 A: `JobState=COMPLETED` かつ `NodeCnt` が 1 以上の行だけを対象にし、残りは件数だけ `SKIPPED` で出す
- 案 B: 状態によらず `NodeCnt` が 1 以上の行を対象にし、状態の列を足す
- 案 C: `CANCELLED` で `NodeCnt=0` の行だけを除く

## 決定と理由

採用: 「案 A」。理由は、見積もりは「最後まで走ったらどれだけかかるか」の予想なので、`FAILED` や `TIMEOUT` の途中までの
実績と比べると比が実際より小さく出るため。除いた件数は `SKIPPED` で見せ、黙って落とさない。案 B・C は、途中で終わった
実績が比に混ざる。決めたのはユーザーの依頼を受けた Claude。

- 実績は `EndTime - StartTime`、見積もりは `TimeLimit`（分）を秒にした値、比は実績 / 見積もり。1 を超えれば見積もりより長くかかった
- `TimeLimit` が数値でない（`UNLIMITED` など）行は見積もりなしとし、比を `-` にする
- ジョブ名ごと（`--by-name`）の比は、実績の合計 / 見積もりの合計（長いジョブに重みが乗る）。個々の比の最大を `MAX_RATIO` に出す。
  ESTIMATE と ELAPSED は平均
- 読めない行（必要な欄が無い、時刻が壊れている）は、行番号を出して失敗する。黙って 0 件扱いにしない
- 実装は bash と awk だけで、`.claude/scripts/slurm-history.sh` に置く

### 帰結

- 良い点: 見積もりの妥当性が、最後まで走ったジョブだけで読める。外部の依存が増えない
- 悪い点: `FAILED` や `TIMEOUT` のジョブは表に出ない（`SKIPPED` の件数だけ分かる）。`--comment` は jobcomp に残らないので、
  ブランチやセッションとの対応は取れない
- 見直す条件: 途中で終わったジョブの実績も見たくなったとき（案 B）

### 確認

`.claude/tests/scripts/slurm-history.sh` で、比の値、`NodeCnt=0` と非 COMPLETED の除外、日付をまたぐ実績、
`--by-name` の集計、読めない行での失敗を確かめる。
