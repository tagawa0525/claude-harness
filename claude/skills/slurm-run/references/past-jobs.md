# 過去のジョブの測定を使う

過去のジョブの測定（時間、`MaxRSS` などのメモリ）を、見積もりに使う、「前は同じ入力で X だった」と比べる、依頼の前提に書くときは、
取るジョブが、コマンドと中身の同じ排他のジョブかを確かめる。`slurm-history.sh --by-name` の表は名前でしか結ばず、sacct の無い
古い jobcomp のジョブには、`-shared` を付け忘れた排他でないジョブも混ざる。

## ジョブを探して読む

```bash
sacct -S <日付> -X -P -n -o JobID,JobName,State,Elapsed,Submit,Start,End,WorkDir,SubmitLine | grep -F -- '<コマンドの一部>'
~/.claude/scripts/slurm-history.sh --job [--repo <本体のリポジトリ>] <番号>…
```

sacct で番号を探し、`--job` に渡す（sacct はジョブ 126 より前を持たない）。`--job` はジョブごとに `STATE:`、`ELAPSED:`、`TIMES:`、
`EXCLUSIVE:`、`WORKDIR:`、`BRANCH:`、`COMMIT:`、`REPO:`、`VERSION:`、`COMMAND:`（2 行目からは `COMMAND+`）の行を出す（行の意味と
版の決め方は `~/.claude/scripts/lib/slurm-job.sh` の先頭）。

- 比べるのは、`STATE: COMPLETED` で `EXCLUSIVE: yes` のジョブ。`-c` のジョブ（`no (--cpus-per-task=N)`）の時間は、同じ仕事なら
  上限としてだけ使う
- `VERSION:` が hash なら走った版で、`NOTE:` の行を、確かめていないこととして添える
- `VERSION:` が `-` なら走った版を決められないので、「確かめていない」と書き、`REASON:` の行を理由に添える。本体のリポジトリが
  見つからなければ（`REASON:` に出る）、`--repo` で渡す

## 中身が同じかを確かめる

- コマンドが同じでも、中で走る仕事（ビルドの有無、作り直す出力の数）が違えば時間は数倍変わる。ログ（SKILL.md の「ログと時間」）の
  進みの行を数えて比べる
- コマンドが読む入力のファイル（TOML など）は、比べる 2 本それぞれで、そのジョブが走った worktree（`WorkDir`）とコミットの版を
  開き、中身が同じかを確かめる（名前やコマンドが同じでも、中身は別でありうる）。`git show <コミット>:<worktree の直下からのパス>`
  で開く（コマンドの中のパスは `WorkDir` からの相対で、`WorkDir` が worktree の下のディレクトリのこともある）
- コマンドが、別のファイルの定義から仕事を組み立てるツール（`make`、パイプラインのツール）なら、その定義の全体と、定義が呼ぶ
  コマンドの読むファイルの全部を、2 本それぞれの版で開いて比べる。読み方はそのリポジトリの規則に従う
- 比べたファイルを書き、上流の仕事の出力など比べていないものがあれば「入力の範囲は確かめていない」と添える

`VERSION:` が hash でも、次のときは走った版を決められないので「確かめていない」と書く（`--job` は判定しない）。

- 入力がそのコミットに無い（git で無視される試行の置き場など）
- ログの先頭に nix の `Git tree '…' is dirty` が出ている
- git で追跡しない入力（ビルドした実行ファイル、ほかの計算の出力など）で、ジョブが使った中身を示す記録（ジョブが書いて
  コミットした md5 の一覧など）が無い

## メモリを比べる

`MaxRSS` は `sacct -j <番号>.batch -o Elapsed,MaxRSS` の `.batch` の行で読む。r995 は 30 秒ごとの標本
（`JobAcctGatherFrequency = 30`）で、COMPLETED のジョブでも間の山は入らない。途中で殺されたジョブ（State が COMPLETED でない）の
値は、殺されるまでの標本で、要る量の下限としてだけ使う。
