---
name: harness-deploy
description: claude-harness の PR をマージした後に、lock の更新、rebuild、claude-sync で配備し、配備された版を確かめる。post-merge-cleanup.sh が NOTE:（rebuild か claude-sync が要る）を出したときに使う。
---

# ハーネスの配備

マージした変更を、この機械の `~/.claude`、hook、git hook に入れる。ユーザーに頼まず自分で実行する（sudoers の NOPASSWD で
通る）。複数の PR を続けてマージするときは、最後のマージの後に 1 度でよい。

## 何を実行するか

`post-merge-cleanup.sh` の `NOTE:` の行で決める。

| NOTE: の行                   | 変わったもの                                     | 実行するもの               |
| ---------------------------- | ------------------------------------------------ | -------------------------- |
| lock の更新と rebuild が要る | `crates/`、`nix/`、`claude-settings.json`、flake | rebuild と claude-sync     |
| claude-sync が要る（だけ）   | `claude/`、`config/`、共通の辞書の規則           | claude-sync と lock の更新 |

`claude-sync` だけで効く変更（スキル、スクリプト、CLAUDE.md、`crates/ja-terms/prh.yml` の規則）でも、lock は進める。lock が古いと、
次の rebuild の activation が lock の版の中身に戻す。

## 手順

1. main が origin/main と一致していることを確かめる（`git -C ~/nix/claude-harness status -sb`）
2. 配備する。`run_in_background=true` の Bash で 1 本にし、終わりの通知を待つ（数分のビルドがある）

   ```bash
   set -o pipefail && nix-rebuild update claude-harness && claude-sync ~/nix/claude-harness
   ```

   - 排他の `timed` の Slurm のジョブが走っている間は hook（guard-exclusive-slurm）が止める。その指示に従う
   - lock が既に進んでいれば「No changes to commit」で終わる（正常）
3. 確かめる

   ```bash
   grep -A12 '"claude-harness"' ~/nix/nixfiles/flake.lock | grep '"rev"'   # マージコミットの hash
   cmp ~/.config/ja-terms/prh.yml ~/nix/claude-harness/crates/ja-terms/prh.yml  # 共通の辞書が main と同じ
   ```

   変えた hook かスクリプトを、変更が見分けられる入力で 1 度動かす（新しく拒否する形、新しい出力の行）。バイナリへの
   grep は、文字列が圧縮や分割で見つからないことがあり、確認にならない
4. ほかのセッションが配備を待っていれば知らせる

## 止まったとき

- `Harness contract check failed`: nixfiles の `modules/home/parts/tests/harness-contract.sh` が、進めた lock の版のハーネスを
  動かして、nixfiles との約束（自動コミットの件名、Slurm の名前、目印のファイル）を確かめている。出力の `✗` の行が食い違いで、
  lock は元に戻されている（配備は前の版のまま）。ハーネスの側が約束を変えたなら nixfiles を合わせる PR を先に出し、
  検査の側の前提が古ければ検査を直す
- ビルドの中のテストの失敗: 手元の `run-checks.sh` と CI が通っていれば、Nix のサンドボックスでだけ落ちる形（PATH に git が無い、
  ホームに辞書が無いなど）を疑う。同じ失敗は `nix flake check` で再現する

## ほかの機械

- NixOS の機械は、次の `nix-rebuild update`（毎日の自動更新を含む）で lock が進み、activation が `claude-setup sync` を呼ぶ
- Windows の機械は /harness-install の更新の手順
