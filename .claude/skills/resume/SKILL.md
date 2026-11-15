---
name: resume
description: セッションの開始、compact の後、/clear の後に、開いている PR・worktree・Slurm のジョブ・docs/issues の issue をスクリプトで集めてから動く。
allowed-tools:
  - Bash(~/.claude/skills/resume/scripts/resume-state.sh)
  - Bash(~/.claude/scripts/git-info.sh)
  - Bash(~/.claude/scripts/slurm-queue.sh)
  - Read
  - Glob
  - Grep
---

# Resume

作業の状態は memory の引き継ぎファイルに書かず、issue と計画（docs/plans）に置く。再開時は、書いてあるものを集めてから動く。

## 現在の状態

!`~/.claude/skills/resume/scripts/resume-state.sh`

## 手順

1. 上の出力を読む。`session=` が自分のセッションでない worktree とブランチは、担当が別なので触らない（引き受けず、担当かユーザーに回す）
2. `docs/plans/` があれば、「進め方」の次の項目を読む。`blocked-by` のある issue は、塞いでいる issue が閉じるまで着手しない
3. Slurm のジョブがあれば `~/.claude/scripts/slurm-queue.sh` で待ち行列を見る。AWS の監視などプロジェクト固有の状態は、そのリポジトリの README か計画に従って集める
4. 次にやること、前提、未決の判断が issue か計画に書かれていなければ、先にそこへ書く（memory には書かない）
5. 集めた状態をユーザーに 3〜5 行で報告し、次の 1 手を示す

## 判断

- 状態の収集で分かるのは事実だけ。次の順序と優先は、計画と issue から読む
- 出力に無いもの（実行中の担当、未決の判断）は、issue か計画に書かれていなければ失われたものとして、ユーザーに聞く
