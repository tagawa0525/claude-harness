---
name: delegate
description: サブエージェントへの委任の定型。受ける側（担当）が最初に呼び、references/working.md の進め方（worktree からチェック・レビューまで進め、1 時間を超える待ちの前で終わり、報告は最後に 1 度）に従う。頼む側（親）は呼ばずに references/requesting.md（依頼に必ず入れる規則）を Read で読む。
# model 未指定: モデルは親がサブエージェントを起動するときに決める（Agent の model）
argument-hint: <リポジトリ> <ブランチ名> <何をするか> <前提> <完成の形> <変える範囲>
allowed-tools:
  - Bash(git *)
  - Bash(gh *)
  - Bash(cd *)
  - Bash(~/.claude/scripts/worktree-add.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Bash(~/.claude/scripts/base-moved.sh*)
  - Bash(~/.claude/scripts/quote-in-transcript.sh*)
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
1 行目の「その手順」はそこにある）。

頼む側（親のセッション）は、依頼を書く前に `~/.claude/skills/delegate/references/requesting.md` を Read で読む。

受ける側の手順をここに置かないのは、親が Skill で呼んだときにその全体が親の文脈に入らないため（親の文脈はセッションが続く限り
毎回読み直され、担当の文脈より高くつく）。頼む側の書き方を置かないのは、担当が読まずに済むため。
