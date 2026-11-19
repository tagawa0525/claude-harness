# claude-harness

このリポジトリだけの規則。全プロジェクトに配る指示は `claude/CLAUDE.md`（`~/.claude/CLAUDE.md` の元）にあり、ここには書かない。
このリポジトリだけのスキルは `.claude/skills/` に置く（配る元の `claude/skills/` とは別で、`claude-setup sync` は配らない）。

- マージした変更は、配備するまで `~/.claude` と hook に効かない。PR をマージしたら /harness-deploy で配備する
- worktree で `claude/` の指示やスキルを直しても、そのセッションには効かない（読むのは `~/.claude`）。試すときは
  `claude-sync <worktree>` で入れ、試し終えたら引数なしの `claude-sync` で main の版に戻す（ほかのセッションにも効く）
- 新しい機械に入れるときと、入れた機械の更新は /harness-install
