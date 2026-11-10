---
name: delegate
description: サブエージェントへの委任の定型。担当が最初に呼び、~/.claude/skills/delegate/references/working.md の手順に従う。頼む側は呼ばずに同じ場所の requesting.md を Read で読む。
# model 未指定: モデルは親がサブエージェントを起動するときに決める（Agent の model）
argument-hint: <リポジトリ> <ブランチ名> <何をするか> <前提> <完成の形> <変える範囲>
allowed-tools:
  - Bash(git *)
  - Bash(gh *)
  - Bash(cd *)
  - Bash(~/.claude/scripts/worktree-add.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Bash(~/.claude/scripts/base-moved.sh*)
  - Bash(~/.claude/scripts/kept-sentences.py*)
  - Bash(~/.claude/scripts/slurm-run.sh*)
  - Bash(squeue*)
  - Bash(sleep *)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh*)
  - Bash(~/.claude/skills/gh-pr-review/scripts/*)
  - Skill(code-review)
  - Skill(adr)
  - Read
  - Edit
  - Write
  - Glob
  - Grep
---

# Delegate

受ける側（担当）は、作業の前に `~/.claude/skills/delegate/references/working.md` を Read で読み、その手順に従う（依頼の
「/delegate を呼び、その手順に従って進める」の手順はそこにある）。会話が圧縮されて読んだ本文が要約に置き換わったら、続ける前に
読み直す（圧縮の後に付け直されるのは呼んだスキルのこの案内で、Read の結果は残るとは限らない）。

頼む側（親のセッション）は、依頼を書く前に `~/.claude/skills/delegate/references/requesting.md` を Read で読む。

受ける側の手順をここに置かないのは、親が Skill で呼んだときにその全体が親の文脈に入らないため（親の文脈はセッションが続く限り
毎回読み直され、担当の文脈より高くつく）。頼む側の書き方を置かないのは、担当が読まずに済むため。
