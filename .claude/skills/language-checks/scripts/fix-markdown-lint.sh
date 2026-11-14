#!/bin/sh
# fix-markdown-lint.sh — markdownlint --fix の直せない MD040（コードの塊の言語）と MD060（全角を考慮した表の桁揃え）を
# 直す。中身は claude-hooks script fix-markdown-lint（Rust にして Windows でも python3 なしで動かす。修正方法と幅の
# 数え方もそこに書く）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: fix-markdown-lint.sh <file>...
#
# 出力: ファイルごとに、直して書いたら `Fixed: <file>`、直すものが無ければ `OK: <file>`
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script fix-markdown-lint "$@"
