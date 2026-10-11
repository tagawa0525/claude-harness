---
status: accepted
date: 2026-10-11
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0016: worktree-add.sh は .worktreelink に書いたリテラルのパスを、メイン worktree への symlink として共有する

## 背景

xlc では、git で追跡しない `references/`（核データ、道具、DVC のキャッシュ `references/dvc-cache`。xlc の ADR-0123）を
worktree の間で共有する必要がある。worktree に `references` が無いと、DVC が worktree の中に別のキャッシュを黙って作り、
ENDF を deps にする段階が止まる。今は xlc の docs/environment.md の手順で、人が `references` をメイン worktree への
symlink にしている。

git 本体には、新しい worktree に追跡しないファイルを持ち込む機能は無い。`worktree-add.sh` は汎用のスクリプトなので、
共有するパスはリポジトリの側に書く。

## 決定の要因

- 大きなデータ（数 GB の核データとキャッシュ）をコピーしない。キャッシュは 1 か所に集める
- symlink が未追跡ファイルに見えて、`git add` でコミットされない
- 書き方の誤りを推測で補わず、エラーで止める

## 検討した案

1. `.worktreeinclude`（Claude Code、Conductor が使う。`.gitignore` の書き方で、無視されたファイルをコピーする）
2. `.worktreelink`（PostHog Desktop が `.worktreeinclude` と並べて使う。`.gitignore` の書き方で、無視されたファイルを
   symlink にする）
3. 独自の名前（例 `.worktree-share`）
4. `worktree-add.sh` に共有するパスを決め打ちする（`references`）

## 決定と理由

案 2 の名前 `.worktreelink` を、書き方を絞って使う。

- 名前: 「無視されたものを symlink にする」という意味が PostHog Desktop と同じなので、その名前を借りる
  （[PostHog Desktop worktrees](https://posthog.com/docs/posthog-desktop/worktrees)）。案 1 は意味がコピーで、
  Claude Code 自身が `.worktreeinclude` を読んで worktree にコピーするため、`references` を書くと数 GB をコピーされる。
  案 3 は意味の同じ既存の名前があるのに増やすことになる。案 4 は xlc のためにスクリプトを汎用でなくする
- 書き方: 1 行 1 パスのリテラルの相対パス。空行と `#` で始まる行は読み飛ばし、末尾の `/` は除く。絶対パス、`.` と `..`
  の要素、ワイルドカード（`*` `?` `[`）、`!` で始まる行はエラーにする。PostHog は `.gitignore` の書き方だが、symlink を
  作るには名前が 1 つに決まる必要があり、パターンを展開する先（メイン worktree のどこに一致するか）を決める必要が
  今は無い。リテラルのパスは `.gitignore` の書き方の部分集合として読める
- 同じパスや入れ子のパス（`references` と `references/sub`）はエラー。後の `ln -s` が先の symlink をたどり、
  メイン worktree の共有するデータの中に symlink を作ってしまう。親が新しい worktree でディレクトリでない
  （ファイルか symlink の）パスもエラーにし、検査をすべて通ってから symlink を作る
- 設定は新しい worktree（チェックアウトしたブランチ）の `.worktreelink` を読む。`.gitignore` と同じブランチのものと
  比べるため
- 新しい worktree で `git check-ignore` に無視されないパスはエラー。symlink はディレクトリではないので、`references/` の
  ようにディレクトリだけに一致するパターンでは無視されない（実体のディレクトリがあるメイン worktree では無視される）。
  そのため判定はメイン worktree でなく、新しい worktree のパスが無い状態で行う。git から見て symlink と同じ扱いになる
- エラーのときは symlink を 1 つも作らずに止め、worktree は残す。直す先はそのブランチの `.worktreelink` か
  `.gitignore` で、その worktree で直せるようにする。worktree を消して戻す案は採らない。誤りがブランチの上に
  あると、そのブランチの worktree を作れず、直すには別の場所で直す必要がある。また `--carry-changes` で
  持ち込んだ変更を、消す前に元へ戻す手順が要る。残す代わりに、`worktree-add.sh` をもう一度呼んでも作成先が
  あるのでエラーになり、直した後の symlink は手で張る
- メイン worktree にパスが無ければ、作らずに `NOTE:` で知らせる。データを取ってくる前のリポジトリでも worktree を
  作れるようにする
- symlink の先は絶対パス（`<メイン worktree>/<path>`）。worktree の置き場所が変わっても指す先が変わらない

### 帰結

- `post-merge-cleanup.sh` は変えない。無視された symlink は `git worktree remove`（`--force` なし）の妨げにならず、
  消すのは symlink だけで、先の実体は残る。worktree を作った後に `.gitignore` が変わって symlink が無視されなく
  なると、未追跡ファイルとして `git worktree remove` が止まる。`--force` で黙って消さず、止まって気づける方を採る
- xlc は `.worktreelink` に `references` を書き、docs/environment.md の手作業の手順を消す（xlc の別の作業）
- `claude --worktree` など、`worktree-add.sh` を通らずに作った worktree には symlink ができない

### 確認

`.claude/tests/scripts/git-workflow.sh` の `worktree-add: .worktreelink ...` と
`post-merge-cleanup: .worktreelink で共有した symlink ...` のテスト。
