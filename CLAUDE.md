# claude-harness

このリポジトリだけの規則。全プロジェクトに配る指示は `claude/CLAUDE.md`（`~/.claude/CLAUDE.md` の元）にあり、ここには書かない。
このリポジトリだけのスキルは `.claude/skills/` に置く（配る元の `claude/skills/` とは別で、`claude-setup sync` は配らない）。

- マージした変更は、配備するまで `~/.claude` と hook に効かない。PR をマージしたら /harness-deploy で配備する
- worktree で `claude/` の指示やスキルを直しても、そのセッションには効かない（読むのは `~/.claude`）。試すときは
  `claude-sync <worktree>` で入れ、試し終えたら引数なしの `claude-sync` で main の版に戻す（ほかのセッションにも効く）
- 新しい機械に入れるときと、入れた機械の更新は /harness-install
- `crates/` の下を変える作業を /delegate で頼むときは、依頼の「完成の形」に「PR の Windows の CI の pass を確かめてから
  マージの許可を求める」と書く。手元のチェックは Linux だけで、Windows でだけ落ちるテスト（パスの区切りと引用符）は PR の CI で
  初めて分かる。許可の後に分かると、修正と確認がもう 1 往復かかる
