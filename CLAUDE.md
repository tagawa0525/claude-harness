# claude-harness

このリポジトリだけの規則。全プロジェクトに配る指示は `claude/CLAUDE.md`（`~/.claude/CLAUDE.md` の元）にあり、ここには書かない。

- `crates/` の下のテストを変えた PR は、PR の Windows の CI（`.github/workflows/windows.yml` の cargo-test）の pass を
  確かめてからマージに進む（/delegate の担当は、それからマージの許可を求める）。テストがパスを出力と比べるかコマンドの
  文字列に埋める形は Linux でだけ通ることがあり、`crates/claude-hooks/tests/test_source_paths.rs` の検査が見つけるのは
  その一部
