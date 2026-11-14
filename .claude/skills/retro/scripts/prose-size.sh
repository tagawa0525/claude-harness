#!/bin/sh
# prose-size.sh — 区間の前後の文章（メモリ、スキル、CLAUDE.md）の大きさをバイトで出す。中身は
# claude-hooks script prose-size（Rust にして Windows でも date と awk に頼らずに動かす）
#
# Usage: prose-size.sh [--since <日時>] [--until <日時>] [--memory <メモリのディレクトリ>] [--repo <パス>]...
#   --since, --until
#               区間 [since, until)。日時は ISO 8601 で、時差（Z か +09:00 の形）を必ず付ける（シェルの TZ で意味が
#               変わらないため）。--since を省くと JST の今日の 0 時
#   --memory    自動メモリのディレクトリ（~/.claude/projects/<プロジェクト>/memory）。前の大きさは ~/.claude の git の
#               refs/memory-backup の、since より前の最後の退避で数える。後の大きさは、--until があれば until より
#               前の最後の退避、無ければ今のディスクのファイルで数える（区間の終わりの退避より後に書いたメモリも入れる）
#   --repo      リポジトリか、その worktree。前の大きさは既定ブランチ（origin/HEAD、無ければ main か master）の
#               first-parent で since より前の最後のコミットで数える。後の大きさは、--until があれば同じ既定
#               ブランチの until より前の最後のコミット、無ければ既定ブランチの今の大きさに、渡したパスの HEAD が
#               既定ブランチとの分岐点から変えた分を足す（worktree を渡せばまだマージしていないブランチの分も入り、
#               遅れた checkout を渡しても既定ブランチにマージした分は落ちない。両方が同じファイルを変えていれば、
#               HEAD の側の変更を既定ブランチの変更に重ねて数える）。作業ツリーの未コミットの変更は数えない
#
# 数える範囲（機械の hook と script は数えない）:
#   memory     メモリのディレクトリの下のすべてのファイル（退避と同じ範囲）
#   skills     リポジトリの .claude/skills/ の下の *.md
#   claude-md  リポジトリのどこかにある CLAUDE.md
#
# 出力:
#   SINCE: <JST の日時>
#   UNTIL: <JST の日時 | now>
#   PROSE: <memory|skills|claude-md> <前> <後> <差（+N、-N、0）> <渡したパス>
#
# メモリのディレクトリも退避も無ければ（メモリの無いプロジェクト）0 と数える。メモリのディレクトリか退避の中身が
# あるのに since より前の退避が無いとき（退避を始める前の区間）と、~/.claude の外のディレクトリを渡したときは、
# 前の大きさが分からないので、ほかの行を出した後に ERROR: を標準エラーに出して exit 1（0 と出すと、増えた分を
# 区間の増分と読む）。

. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script prose-size "$@"
