---
name: history-rewrite
description: リポジトリの履歴の中の崩れた語を ja-terms の辞書で直し、文書のコミットの番号を付け直して、生きているリポジトリと worktree に移す。履歴を書き換えるとき（過去のコミットの語を直す、記録の番号を付け直す）に使う。
argument-hint: <リポジトリのパス>
allowed-tools:
  - Bash(history-rewrite run:*)
  - Bash(history-rewrite transform:*)
  - Bash(history-rewrite sync-worktrees:*)
  - Bash(commit-refs remap:*)
---

# 履歴の語を直す

`history-rewrite`（`crates/history-rewrite/`）で、対象のリポジトリ（$ARGUMENTS）のすべてのブランチとタグの語を
辞書で直し、番号を付け直す。ツールは作業ディレクトリで書き換えて照合し、対象のリポジトリには書き込まない。
生きているリポジトリに移すのは 4 から。

すべてのコミットの番号が変わるので、始める前に、ユーザーに書き換えることを確かめる。

## 1. 誰も作業していないことを確かめる

```bash
git -C <リポジトリ> worktree list
squeue   # そのリポジトリの計算が走っていないか
```

worktree ごとのブランチの `Claude-Session:` を見て、作業中のセッションが無いことをユーザーに確かめる。走っている
セッションや計算があれば、終わるまで待つ（途中で増えたコミットは書き換えに入らない）。

## 2. 語の指定を決める

- 辞書: 省くと ja-terms の辞書。辞書に無い 1 回限りの置き換えは、prh の形のファイルにして `--extra-rules` で足す
  （`version: 1` と `rules:` の下に `expected` と `pattern`）
- 除くパス（`--exclude`、PurePosixPath.full_match の書き方）: 辞書の語を例として挙げるファイル。対象のリポジトリの
  `.textlintignore` に挙げたファイルと、辞書そのもの（claude-harness なら `**/prh.yml`）を渡す。語を論じる文書でも、
  その語が直す対象の造語なら除かない

## 3. 書き換えて照合する

作業ディレクトリは、対象のリポジトリの外の、空のディレクトリにする。中のミラー（`original.git`）は書き換える前の
控えなので、7 の issue でユーザーが決めるまで残す。

```bash
history-rewrite run --source <リポジトリ> --workdir <作業ディレクトリ> <2 で決めた語の指定>
```

`ERROR:` で止まったら、理由を直してから空の作業ディレクトリでやり直す（照合が合わないときはツールの不具合なので、
issue に書く）。通ったら、直した中身を読む:

```bash
cd <作業ディレクトリ>/rewritten
old=$(git -C ../original.git rev-parse main); new=$(git rev-parse main)
GIT_ALTERNATE_OBJECT_DIRECTORIES=$PWD/../original.git/objects git diff --stat "$old" "$new"
```

直り方が意図と違えば、辞書か除くパスを直して 3 をやり直す。

## 4. 生きているリポジトリに移す

以下では `R` が対象のリポジトリ、`D` が作業ディレクトリ。

元のリポジトリのブランチとタグが、ミラーを取ったときから動いていないことを確かめる。動いていれば 1 からやり直す:

```bash
diff <(git -C "$R" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/tags) \
  <(git -C "$D/original.git" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/tags)
```

ref を移す前に、各 worktree の HEAD と、変更の控えのパッチを残す（5 のツールが、この HEAD と比べて未コミットの
変更を見つける）:

```bash
# パスは空白を含みうるので、行の残り全体を取り、列はタブで区切る。控えのパッチは行の番号で名付ける
git -C "$R" worktree list --porcelain | sed -n 's/^worktree //p' >"$D/worktrees"
k=0
while IFS= read -r w; do
  k=$((k + 1))
  git -C "$w" diff HEAD >"$D/worktree-$k.patch"
  printf '%s\t%s\t%s\n' "$w" "$(git -C "$w" rev-parse HEAD)" "$D/worktree-$k.patch"
done <"$D/worktrees" >"$D/worktree-state"
```

オブジェクトを取り込み、すべてのブランチとタグを 1 つのトランザクションで移す。各行は「元の番号のままなら」の
条件つきで、動いた ref が 1 つでもあれば、どれも動かない:

```bash
# refs/heads/ には直接 fetch しない（チェックアウト中のブランチを fetch が拒み、--update-head-ok では条件つきの更新にならない）
git -C "$R" fetch -q --no-tags "$D/rewritten" \
  '+refs/heads/*:refs/history-rewrite/heads/*' '+refs/tags/*:refs/history-rewrite/tags/*'
git -C "$D/original.git" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/tags |
  while read -r ref old; do echo "update $ref $(git -C "$D/rewritten" rev-parse "$ref") $old"; done >"$D/transaction"
git -C "$R" for-each-ref --format='delete %(refname)' refs/history-rewrite/ >>"$D/transaction"
# 条件つきの行は、トランザクションに無い ref が増えたことを見ない。移す直前にもう 1 度比べる
diff <(git -C "$R" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/tags) \
  <(git -C "$D/original.git" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/tags) &&
  git -C "$R" update-ref --stdin <"$D/transaction"
```

比べてから移すまでの間に作られた ref は残りうる（1 で作業中のセッションが無いことを確かめるのは、このため）。

## 5. worktree を合わせる

ref を移した後、各 worktree の index と作業ツリーは元の番号のまま残る。run が作業ディレクトリに記録した語の
指定で合わせる:

```bash
history-rewrite sync-worktrees --workdir "$D"
```

worktree ごとの行の後の `NEXT:` に従う（`NEXT:` の無い `RESET:` と `SKIPPED:` は対応不要。追跡していないファイルは
残る）。

## 6. 書き換えの外に残るもの

- `refs/remotes/`、`refs/stash`、notes は移さない。リモートに送るには force push が要るので、ユーザーが決める
  （GitHub のバックアップのリモートも同じ）
- 対象のリポジトリが DVC を使うときは `~/.claude/skills/history-rewrite/references/dvc.md` を読む
- ほかのリポジトリやメモリの中の、このリポジトリの番号は、そのリポジトリで `commit-refs remap "$D/commit-map"` で
  付け直す

## 7. 古いオブジェクトの掃除を issue にする

古いオブジェクトの掃除と作業ディレクトリ（ミラーの控え）の削除は取り消せないので、ここでは行わず、3 の run の
出力の `ISSUE:` のファイル（1 行目の見出し（`#` の後）が題、残りが本文）で対象のリポジトリの issue を開き、ユーザーが決める。
