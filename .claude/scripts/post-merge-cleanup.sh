#!/bin/sh
# post-merge-cleanup.sh — PR マージ後（または /git-merge の後）にブランチと worktree を片付ける。中身は claude-hooks script
# post-merge-cleanup（Rust にして Windows でも動かす）。スキルの文はこのスクリプトの名前とパスで呼ぶので、ファイルは
# 残して呼ぶ
#
# Usage: post-merge-cleanup.sh <branch>
#
# 順に行う（gh-pr-merge スキルの「クリーンアップ」）:
#   -. （0 の前）cwd のリポジトリに <branch> の痕跡が無ければ、何も変えずに ERROR で止まる（別のリポジトリの
#      ブランチ名を渡すと、none を並べて 0 で終わり、そのリポジトリの pull と予約のタグの prune だけが走る）。
#      痕跡は、ローカルのブランチ（チェックアウトした worktree を含む）、リモート追跡の ref、リモートのブランチ、
#      GitHub リモートではそのブランチを head とするマージ済みの PR。gh pr merge --delete-branch はローカルと
#      リモートのブランチを消し、リモート追跡の ref もほかのセッションの fetch --prune で消えうるので、
#      マージ済みの PR まで見ないと、gh の後の正当な呼び出しを止める。確かめられない（リモートに届かない、
#      gh が失敗する）ときも止める
#   0. （1 の前）ローカルの <branch> の先端が既定ブランチ（なければ、追うリモートの既定ブランチを fetch した
#      もの）の祖先でなければ、何も消さずに ERROR で止まる（マージが途中で止まったのに続けて呼ぶと、1 で worktree
#      が、無視されたファイルごと消える。4 の -d で未マージに気づいても遅い）。ローカルのブランチが無ければ見ない
#   1. <branch> をチェックアウトしている worktree があれば削除（変更が残っていれば失敗する）
#      どの worktree から実行しても、既定ブランチの worktree に移ってから行う
#   2. 既定ブランチ（main / master）へ切り替え、fetch --prune、pull --ff-only（リモートがなければ省く）
#      （ローカルとリモートの履歴が分岐していれば ff できずに失敗し、気づける）
#      pull で配置するものが変われば NOTE で知らせる。ハーネス（同期のコマンドの claude-setup か claude-sync.sh を持つ）では、modules か flake なら取り込む側の
#      lock の更新と rebuild、.claude なら claude-sync。rebuild する構成のリポジトリ（nix-rebuild.sh を持つ）では、
#      modules、hosts、secrets、flake なら rebuild。両方を持つリポジトリ（自分の .claude を配り、自分で rebuild する）では、
#      rebuild と claude-sync
#   3. origin から予約のタグ（adr-NNNN、issue-NNNN。ほかの clone が push したものを含む）を取ってきて、あれば、
#      既定ブランチの履歴に記録が入った番号のタグを、ローカルとリモートから消す（design-records prune）。
#      design-records が無いか origin に届かなければ残して続け、prune が失敗したら ERROR で止まる。
#      既定ブランチが origin を追うときは、origin に届かなければ 2 の fetch で止まる（既定ブランチを最新にできず、
#      5 のリモートブランチの削除もできないので、続けない）。3 で残して続けるのは、既定ブランチがほかのリモート
#      （fork の upstream）を追うとき
#   4. ローカルブランチを削除（-d。0 で確かめたあとの安全網）
#   5. リモートブランチを削除。ただし、リモートの既定ブランチがブランチの先端を含まないとき（push 前）と、
#      GitHub リモートでそのブランチを head とする open PR（fork からの upstream PR 等）があるとき
#      （削除で PR が閉じる）は、削除せず残す
#
# 出力:
#   NOTE: <branch> は手元にもリモートにも無く、マージ済みの PR <url> を痕跡として...（マージ済みの PR だけが痕跡のとき）
#   WORKTREE_REMOVED: <path>|none
#   NOTE: rebuild が要る（...）。... / NOTE: lock の更新と rebuild が要る（...）。... / NOTE: claude-sync が要る（...）。...
#     （該当するときだけ。0 行以上）
#   RESERVATIONS_RELEASED: <tag>...|none|skipped (...)|kept (...)
#   LOCAL_BRANCH: deleted|none
#   REMOTE_BRANCH: deleted|kept (...)|none

. "$(dirname "$0")/lib/claude-hooks-script.sh"
claude_hooks_script post-merge-cleanup "$@"
