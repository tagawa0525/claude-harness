#!/bin/sh
# base-moved.sh — 比較元が分岐点から進み、ブランチと同じパスを触っていないかを知らせる。中身は claude-hooks script
# base-moved（Rust にして Windows でも動かす。fetch するかの決め方と WARN もそこに書く）。スキルの文はこのスクリプトの
# 名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: base-moved.sh [branch] [--base BASE]
#   branch  省略時は今のブランチ
#   BASE    省略時は origin の既定ブランチ（origin/HEAD → origin/main → origin/master）。
#           積み重なったブランチは、GitHub の clone では土台を origin/<土台のブランチ> で渡す（fetch が
#           更新するのは origin/* だけで、ローカルのブランチは古いまま比べることになる。origin 以外の
#           リモートの追跡ブランチは fetch せず WARN）。GitHub の無い clone（リモートの無いリポジトリを
#           含む）ではローカルのブランチ名（--base main）を渡す（GitHub の origin が無く --base も
#           無ければ比べずに UNVERIFIED を出す）
#
# 出力:
#   BASE が分岐点から進み、進んだ変更とブランチの変更が同じパスを触るときだけ:
#     BASE_MOVED: <BASE> が分岐点から <N> commits 進んでいる（先頭 <hash>、分岐点 <hash>）   マージを 1 件と数える
#     COMMON: <両方が触ったパス>   パスごとに 1 行
#     NEXT: git diff <分岐点>..<先頭> -- <パス>… で …   読む差分と、読んだ後の進め方
#   BEHIND: <N>         比較が済んだとき。ブランチにまだ入っていない BASE のコミットの数（マージで入ったものも数える）
#   UNVERIFIED: ...     比較が済んでいない（WARN を出した、比較元が決まらない）とき、BEHIND の代わりに出す
. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script base-moved "$@"
