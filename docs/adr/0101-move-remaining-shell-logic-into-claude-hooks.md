---
type: ADR
status: proposed
date: 2026-11-19
requires: [ADR-0087, ADR-0089]
supersedes: []
superseded-by:
issues: ["#350", "#169"]
description: Slurm の 4 本と check-each-commit.sh の中身を claude-hooks のサブコマンドに移し、sh は呼ぶだけにする。中断の後片付けは ctrlc で受ける
---

# ADR-0101: 残る sh のロジック（Slurm の 4 本と check-each-commit）を claude-hooks のサブコマンドに移し、中断の後片付けは ctrlc で受ける

## 背景

ADR-0087 と ADR-0089 で、作業手順のスクリプトの中身を `claude-hooks` のサブコマンドに移し、sh は呼ぶだけにした。
ロジックが sh に残っているのは次の 5 本（コメントと空行を除いた行数。main の eace5e09、2026-11-19、
`grep -cv '^\s*#\|^\s*$'`）。ほかの 33 本は 2〜3 行で `claude_hooks_script` を呼ぶだけになっている。

| ファイル                              | 行  | 役目                                                                                          |
| ------------------------------------- | --- | --------------------------------------------------------------------------------------------- |
| `claude/scripts/slurm-run.sh`         | 326 | 計算を 1 本投げ、区分の順に並べ直し、終わるまで待って結果を出す                               |
| `claude/scripts/lib/slurm-job.sh`     | 252 | sacct の記録から、排他か、コマンド、走った版を読む（`slurm-history.sh --job` が source する） |
| `claude/scripts/slurm-history.sh`     | 215 | jobcomp と sacct の記録から、見積もりと実績を比べる                                           |
| `claude/scripts/slurm-queue.sh`       | 106 | 待ち行列を区分と目的つきで並べ、並べ直す番号の列を出す                                        |
| `claude/scripts/check-each-commit.sh` | 55  | `<base>..HEAD` の各コミットを一時の worktree で検査する                                       |

- Slurm の 4 本は、ADR-0087 が範囲（Git と GitHub の作業手順）から外した。r995 でだけ使い、Windows では動かさない
- `check-each-commit.sh` は、ADR-0087 が sh に残すと決めた。中断のときに一時の worktree を消す `trap` を、Rust の標準
  ライブラリで書けないため（シグナルを受ける crate か unsafe が要る。#169）
- 2026-11-19、xlc の担当が見積もり 175 分のジョブ（Slurm 961）を `slurm-run.sh` で投げ、終わるまで待つ形のまま
  約 5 分ごとに再開した。待つか終えるかの判定（ADR-0078 の 55 分）はスキルの文にあり、担当も親も守らなかった（#350）
- ユーザーは、機械的に決まる判定は LLM に判断させず、ロジックは sh に書き足さずに Rust のテストできる形に置くことと、
  ほかの sh に残るロジックも Rust に移すことを求めた（ユーザー 2026-11-19 の発言の要約。前半は xlc-e5 のセッションが
  伝えた）
- sh のテストは `claude/tests/scripts/slurm-*.sh` の 5 本、1,754 行で、偽の `squeue` などを PATH に置いて、スクリプトを
  丸ごと起動して確かめている。判定（区分の順、終わった State、版の決め方）を、起動せずに確かめるテストは無い
- CLAUDE.md の依存の原則は、2026-11-17 に「標準ライブラリと、その言語やエコシステムで事実上の標準として広く使われる
  ライブラリとツールを同じ側に数え、手書きと比べて保守コストの小さい方を選ぶ」に変わった（ADR-0087 は 2026-11-13）

## 決定の要因

- 判定を、プロセスを起動せずにテストできること（#350 の待つかの判定は、見積もり、開始の見込み、55 分の線の組み合わせ）
- 判定の置き場が 1 か所であること（区分の一覧は、今は `slurm-run.sh` の case と `slurm-queue.sh` の 2 か所にある）
- スクリプトのパス、引数、出力の行、終了コードを変えないこと（スキル、hook の `guard_sbatch`、`claude-settings.json` の
  許可、xlc のスキルが、パスと行の形で読む）
- 中断されたときの後片付け（一時の worktree、一時ファイル、`RESULT:` の行）を失わないこと
- Windows でも crate がビルドできること（Slurm のサブコマンドは r995 でだけ動けばよい）

## 検討した案

- 案 甲: 5 本とも中身を `claude-hooks script <名前>` に移し、sh は呼ぶだけにする。中断は `ctrlc` crate で受ける
- 案 乙: 判定だけを Rust の関数に移し、sh が投げる・待つ・`trap` を持ち続けて、判定ごとに `claude-hooks` を呼ぶ
- 案 丙: Slurm の 4 本だけを移し、`check-each-commit.sh` は ADR-0087 のとおり sh に残す
- 案 丁: 5 本とも移し、中断の後片付けは crate を足さずに書く（`sigaction` を直に呼ぶ unsafe か、後片付けをやめて
  次の起動で残りを消す）
- 案 戊: 移さず、#350 の判定だけを `slurm-run.sh` に書き足す

## 決定と理由

案 甲を採る。

- 5 本の中身を `claude-hooks script slurm-run`、`slurm-queue`、`slurm-history`、`check-each-commit` に移す。
  `lib/slurm-job.sh` は source される部品なので、`slurm-history` の中に移し、移した PR で消す
- 5 本のうち、直接呼ぶ 4 本のファイルは残し、`lib/claude-hooks-script.sh` を読んで `claude_hooks_script <名前>` を
  呼ぶだけにする（ADR-0087 と同じ形）。パス、引数、出力の行、終了コードは変えない
- `sbatch`、`squeue`、`sacct`、`scontrol`、`scancel`、`git` は子プロセスで呼ぶ。判定（区分の順、並べ直す列、終わった
  State、見積もりの読み方、版の決め方）は、入力の文字列を受けて結果を返す関数にし、単体のテストで確かめる。丸ごとの
  振る舞いは、`claude-hooks` の結合テストの偽のコマンド（`tests/common`）で確かめる
- 中断（INT、TERM、HUP）は `ctrlc`（`termination` の feature）で受け、後片付けをしてから終わる
- 移すだけで、振る舞いは足さない。#350 の待つかの判定は、`slurm-run` を移した後に、別の決定として足す

理由:

- 案 乙は、投げてから待つまでの流れ（`sbatch --wait` を裏で実行し、番号を読み、並べ直し、sacct を待つ）が sh に残る。
  その流れの分かれ目が #350 で変える所で、sh に書き足すことになる
- 案 丙は、`check-each-commit.sh` を残す理由（シグナルを受ける手段が標準ライブラリに無い）が、`slurm-run` を移すと
  同じ形で出る（`slurm-run.sh` は終了の `trap` で `RESULT:` を出し、一時ファイルを消す）。片方のために手段を入れるなら、
  もう片方を残す理由が無い
- 案 丁の unsafe は、Unix と Windows で別の関数を自分で持つことになる。後片付けをやめる形は、中断のたびに一時の
  worktree が残り、次の起動まで `git worktree list` に出る
- `ctrlc` は、crates.io の累計のダウンロードが 1 億 4,474 万、直近が 2,710 万（3.5.3。2026-11-19 に
  `https://crates.io/api/v1/crates/ctrlc` で取得）で、広く使われている。依存の原則の「事実上の標準」に当たると判断した。
  `signal-hook`（0.4.6。累計 3 億 133 万）はシグナルの番号を見分けられるが、要るのは「中断されたら後片付けをする」
  だけで、`ctrlc` の方が小さい
- 案 戊は、判定をテストできない場所に足し、ユーザーの求めと合わない

### 帰結

- `claude-hooks` の依存に `ctrlc` が増える
- 中断されたときの終了コードは、シグナルによらず 130 になる（今の `check-each-commit.sh` は INT 130、TERM 143、HUP 129。
  `ctrlc` はどのシグナルかを渡さない）。この 3 つを見分けて読む呼び出しは、リポジトリの中に無い（確かめるのは実装の PR）
- sh のテスト（`claude/tests/scripts/slurm-*.sh`）のうち、判定を確かめるものは Rust の単体のテストに移り、sh のテストは
  「sh から呼べること」だけを確かめる（ADR-0087 と同じ分け方）
- ADR-0087 の「`check-each-commit.sh` は移さず、sh のまま残す」は、この ADR で変わる（ADR-0087 の補足に書く）
- 実装は、スクリプトごとに 1 つの PR で、次の順に進める（どれも `crates/claude-hooks/src/script/` と
  `claude/tests/scripts/` を変えるので、1 つがマージされてから次に進む）。全部が移るまで、この ADR は proposed のまま、
  移したものに PR の番号を書く
  1. `slurm-queue`（最も小さく、`slurm-run` が並べ直しで呼ぶ。区分の一覧をここに置く）:
  2. `slurm-run`（投げる、並べ直す、待つ、結果を出す。`ctrlc` をここで足す。振る舞いは変えない）:
  3. #350 の待つかの判定（形は別の決定として書いてから実装する。失敗が実際に起きているので、`slurm-history` より先）:
  4. `slurm-history` と `lib/slurm-job.sh`:
  5. `check-each-commit`（この PR で accepted にする）:

### 確認

- `git ls-files '*.sh'` のうち、テストでない sh が、`lib/claude-hooks-script.sh` を除いてすべて 3 行以下（コメントと
  空行を除く）
- `claude-hooks` の単体のテストに、区分の順、並べ直す列、終わった State、版の決め方のテストがある
- r995 で、移した後の `slurm-run.sh` で 1 本投げて、`JOB:`、`TIME`、`RESULT:`、`EXIT:` の行が前と同じ形で出る
- 中断（`kill -INT`）の後に、`check-each-commit` の一時の worktree が `git worktree list` に残らない

## 各案の長所と短所

### 案 甲: 5 本とも移し、中断は ctrlc で受ける

- 良い: 判定を起動せずにテストできる。sh に残るロジックが無くなる
- 良い: 中断の後片付けを、Unix と Windows で同じコードで書ける
- 悪い: 依存が 1 つ増える。中断の終了コードを見分けられなくなる

### 案 乙: 判定だけを移し、流れは sh に残す

- 良い: 依存が増えない。`trap` をそのまま使える
- 悪い: 流れの分かれ目（待つか終えるか）が sh に残り、#350 を sh に書き足すことになる
- 悪い: 判定ごとにプロセスを起動し、sh と Rust の間で値を文字列で受け渡す

### 案 丙: check-each-commit.sh だけ sh に残す

- 良い: 移す量が 55 行ぶん減る
- 悪い: `slurm-run` のためにシグナルの手段を入れた後は、残す理由が無い

### 案 丁: crate を足さずに中断を扱う

- 良い: 依存が増えない
- 悪い: unsafe を OS ごとに持つか、中断のたびに一時の worktree を残す

### 案 戊: 移さない

- 良い: 変更が最も小さい
- 悪い: 判定をテストできない場所に足し続ける
