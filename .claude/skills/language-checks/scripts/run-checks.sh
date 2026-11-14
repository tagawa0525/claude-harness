#!/bin/sh
# run-checks.sh — プロジェクトの言語を検出し、フォーマット → リント → テストを実行する。中身は claude-hooks script
# run-checks（Rust にして Windows でも動かす。言語の検出と段の中身もそこに書く）。スキルの文と git の pre-merge-commit
# hook はこのスクリプトの名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: run-checks.sh [-C <dir>] [--merge] [--base <ref>]
#
#   --merge  マージ結果を検査する（git の pre-merge-commit hook が使う）。「ステージ済み」の代わりに
#            HEAD（マージ先）と作業ツリーの差分で言語と対象を決め、自動修正はしない
#            （直した内容はどちらの親にもない変更としてマージコミットに紛れ込むため、lint で止める）。
#            commit -a でマージを締めるとき hook に渡る index は一時ファイルなので、index に頼らない
#   -C <dir>  <dir> の worktree を検査する（git -C と同じ。今いる場所と別の worktree のブランチを検査するとき）
#   --base <ref>  コミット済みのブランチを検査する（/git-merge がマージの前に実行する）。<ref> との分岐点から
#            作業ツリーまでの差分で言語と対象を決め、--merge と同じく自動修正はしない。
#            ステージ済みの差分は、クリーンなブランチでは空なので使えない
#
# 最初に失敗したチェックで止まり、失敗したコマンドと自動修正コマンド（あれば）を出す。
# ツールがなければ SKIP を出して続行する。排他の Slurm のジョブ中は、そのジョブの計算ノードの上では cargo と
# nix flake check を 1 コアに限り、ほかのホストでは nix のビルドを手元でする（時間を記録するジョブを乱さないため）
#
# 出力:
#   RUN: <lang> <stage>: <command>
#   OK: <lang> <stage> | SKIP: <lang> <stage> (<理由>) | FAILED: <lang> <stage>: <command>
#   FIX: <自動修正コマンド>（FAILED のときのみ、あれば）
#   ALL_OK（すべて通ったとき）
#
# プロジェクトの CLAUDE.md にチェックコマンドが明記されていればそちらを優先する
# （例: feature gate のあるクレートでは --all-features が必要）。ここでの既定値は汎用。
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script run-checks "$@"
