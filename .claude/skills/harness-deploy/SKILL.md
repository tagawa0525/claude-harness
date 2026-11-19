---
name: harness-deploy
description: claude-harness の PR をマージした後に、lock の更新、rebuild、claude-sync で配備し、配備された版を確かめる。post-merge-cleanup.sh が NOTE:（rebuild か claude-sync が要る）を出したときに使う。
---

# ハーネスの配備

マージした変更を、この機械の `~/.claude`、hook、git hook に入れる。ユーザーに頼まず自分で実行する（sudoers の NOPASSWD で
通る）。複数の PR を続けてマージするときは、最後のマージの後に 1 度でよい。

## いつ実行するか

`post-merge-cleanup.sh` が `NOTE:` の行（lock の更新と rebuild が要る、claude-sync が要る）を出したとき。どの `NOTE:` でも下の
手順は同じで、lock を進めて rebuild し、claude-sync で main の版を入れる。スキル、スクリプト、CLAUDE.md、共通の辞書の規則は
`claude-sync` だけで効くが、lock は進める（lock が古いと、次の rebuild の activation が lock の版の中身に戻す）。

## 手順

1. 2 つのリポジトリが main にいて、ハーネスが origin/main と一致していることを確かめる（`nix-rebuild update` は nixfiles が
   main でないと止まる）。どちらの出力も `## main...origin/main` だけで、`[ahead` も `[behind` も無いこと

   ```bash
   git -C ~/nix/claude-harness fetch -q && git -C ~/nix/claude-harness status -sb
   git -C ~/nix/nixfiles status -sb
   ```

2. 配備する。`run_in_background=true` の Bash で 1 本にし、終わりの通知を待つ（数分のビルドがある）

   ```bash
   nix-rebuild update claude-harness && claude-sync ~/nix/claude-harness
   ```

   - lock が既に進んでいれば「No changes to commit」で終わる（正常）
   - rebuild が止まり（下の「止まったとき」）、マージした変更が `claude-setup sync` の配るもの（`claude/`、`config/`、
     共通の辞書 `crates/ja-terms/prh.yml`）だけなら、`claude-sync ~/nix/claude-harness` だけを実行して先に効かせ、rebuild は
     原因を直してからやり直す。ほかの `crates/` か `nix/` の変更を含むときは、スキルと hook の版がずれるので、rebuild が
     通るまで claude-sync も実行しない
3. 確かめる

   ```bash
   jq -r '.nodes["claude-harness"].locked.rev' ~/nix/nixfiles/flake.lock        # マージコミットの hash
   cmp ~/.config/ja-terms/prh.yml ~/nix/claude-harness/crates/ja-terms/prh.yml  # 共通の辞書が main と同じ
   ```

   変えた hook かスクリプトを、変更が見分けられる入力で 1 度動かす（新しく拒否する形、新しい出力の行）。バイナリへの
   grep は、文字列が圧縮や分割で見つからないことがあり、確認にならない
4. ほかのセッションが配備を待っていれば知らせる

## 止まったとき

どの場合も、nix-rebuild は lock を元に戻して終わり、配備は前の版のまま。

- hook（guard-exclusive-slurm）の拒否: 排他の `timed` の Slurm のジョブが走っている。拒否の文言の指示に従う
- `Harness contract check failed` で、出力に `✗` の行がある: nixfiles の `modules/home/parts/tests/harness-contract.sh` が、
  進めた lock の版のハーネスを動かして、nixfiles との約束を確かめている（確かめる約束はそのスクリプトの冒頭）。`✗` の行が
  食い違い。ハーネスの側が約束を変えたなら nixfiles を合わせる PR を先に出し、検査の側の前提が古ければ検査を直す
- 同じ文言で `✗` の行が無い: 進めた lock で、この機械の構成を評価できない（nix の error の行が原因）
- ビルドの中のテストの失敗: 手元の `run-checks.sh` と CI が通っていれば、Nix のサンドボックスでだけ落ちる形（PATH に git が無い、
  ホームに辞書が無いなど）を疑う。同じ失敗は `nix flake check` で再現する

## ほかの機械

- NixOS の機械は、次の `nix-rebuild update`（毎日の自動更新を含む）で lock が進み、activation が `claude-setup sync` を呼ぶ
- Windows の機械は /harness-install の更新の手順
