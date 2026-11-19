---
name: harness-install
description: claude-harness を新しい機械に入れるとき、入れた機械を更新するとき、入れた後に動いているかを確かめるときに使う。
---

# ハーネスの導入と更新

手順の本文は README と `docs/windows.md` にあり、ここには入口と、入れた後の確認だけを書く（手順をここにコピーしない。
コピーは片方だけが直って古くなる）。

## NixOS と home-manager

- 入れる: README の「取り込み方」のとおり、取り込む側の flake に input を足し、home-manager の構成で module を import して
  rebuild する。Claude Code の本体は取り込む側で入れる
- 更新する: 取り込む側で lock を進めて rebuild する（この機械では /harness-deploy）。activation が `claude-setup sync` を呼び、
  `~/.claude` と `~/.config` の配るファイルを置く
- `claude-setup sync` の配るもの（`crates/claude-setup/src/sync.rs` の冒頭に一覧）だけを急いで入れるときは `claude-sync`（引数なしは既定の clone
  `~/nix/claude-harness` を pull してから同期する）

## Windows

- 入れる、更新する: `docs/windows.md` を上から実行する。更新は、clone を pull して同じ手順を再実行する

## 入れた後の確認

```bash
ls ~/.claude/bin                                    # claude-hooks と ja-terms（Windows は claude-setup などの 6 つ）
ls ~/.claude/CLAUDE.md ~/.claude/skills ~/.claude/scripts
ls ~/.config/ja-terms/prh.yml ~/.config/markdownlint/config
git config --global core.hooksPath                  # ~/.config/git/hooks
```

- hook が効いていること: Claude Code のセッションで、止められるはずのコマンドを 1 つ試す（`git add -A` は guard-git-add が
  拒否する）
- 辞書が効いていること: 一時のファイルに、辞書（`crates/ja-terms/prh.yml`）の specs のうち `from` と `to` が違う例の `from` を 1 つ書き、`~/.claude/bin/ja-terms check <ファイル>` が止めることを見る
- どれかが無いか効かなければ、入れる手順のうち、そのファイルを置くステップ（上の入口の文書）に戻る
