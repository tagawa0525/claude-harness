---
name: history-rewrite
description: リポジトリの履歴の中の崩れた語を ja-terms の辞書で直し、文書のコミットの番号を付け直して、生きているリポジトリと worktree に移す。履歴を書き換えるとき（過去のコミットの語を直す、記録の番号を付け直す）に使う。
argument-hint: <リポジトリのパス>
allowed-tools:
  - Bash(history-rewrite run:*)
  - Bash(history-rewrite transform:*)
  - Bash(commit-refs remap:*)
---

# 履歴の語を直す

`history-rewrite`（`modules/home/parts/history-rewrite/`）で、対象のリポジトリ（$ARGUMENTS）のすべてのブランチと
タグの語を辞書で直し、番号を `commit-refs rewrite-history` で付け直す。ツールは対象のリポジトリに書き込まず、
作業ディレクトリで書き換えて照合する。生きているリポジトリに移すのは、この手順の 4 から。

履歴の書き換えは、すべてのコミットの番号を変える。始める前に、ユーザーに書き換えることを確かめる。

## 1. 誰も作業していないことを確かめる

書き換えの途中で増えたコミットは、書き換えに入らない。worktree で作業中のセッションは、番号が変わると食い違う。

```bash
git -C <リポジトリ> worktree list
squeue   # そのリポジトリの計算が走っていないか
```

worktree ごとのブランチの `Claude-Session:` を見て、作業中のセッションが無いことをユーザーに確かめる。走っている
セッションや計算があれば、終わるまで待つ。

書き換えに入るのは、ブランチとタグから届くコミットだけ。detached HEAD の worktree が、どのブランチとタグからも
届かないコミットにあれば、そのコミットにブランチを付けてから進める（付けなければ、その worktree は元の履歴に
残り、5 で合わせられない）。

## 2. 語の指定を決める

- 辞書: 省くと ja-terms の辞書（hook と同じ）。辞書に無い 1 回限りの置き換えは、prh の形のファイルにして
  `--extra-rules` で足す（`version: 1` と `rules:` の下に `expected` と `pattern`。prh.yml と同じ書き方）
- 除くパス（`--exclude`、PurePosixPath.full_match の書き方）: 辞書の語を例として挙げるファイル。対象のリポジトリの
  `.textlintignore` に挙げたファイルと、辞書そのもの（nixfiles なら `**/prh.yml`）を渡す。語を論じる文書でも、
  その語が直す対象の造語なら除かない（除くと、その文書の造語が直らずに残る）

決めた指定は 1 度だけ書き、`run` と 5 の `sync-worktrees` の両方に同じものを渡す（違うと、作業ツリーのファイルだけ
別の変換になる）:

```bash
WORDS=(--extra-rules <ファイル> --exclude '<パス>' ...)
```

## 3. 書き換えて照合する

作業ディレクトリは、対象のリポジトリの外の、空のディレクトリにする。中のミラー（`original.git`）が書き換える前の
控えになるので、古いオブジェクトを消すと決めるまで残す。

```bash
history-rewrite run --source <リポジトリ> --workdir <作業ディレクトリ> "${WORDS[@]}"
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

元のリポジトリのブランチとタグが、ミラーを取ったときから動いていないことを確かめる（動いていれば、新しい
コミットが書き換えに入っていないので、1 からやり直す）:

```bash
diff <(git -C "$R" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/tags) \
  <(git -C "$D/original.git" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/tags)
```

各 worktree の HEAD と、変更の控えのパッチを、ref を移す前に残す（5 のツールは、この HEAD と比べて未コミットの
変更を見つける。移した後の HEAD と比べると、書き換えたファイルが変更に見える）:

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
条件つきなので、途中で動いた ref があれば、どれも動かない:

```bash
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

`git fetch` で `refs/heads/` に直接取り込まないのは、worktree にチェックアウト中のブランチを fetch が拒むため
（`--update-head-ok` で外すと、条件つきの更新にならない）。

## 5. worktree を合わせる

`update-ref` はチェックアウト中のブランチも移すので、各 worktree の index と作業ツリーは元の番号のままになる。
2 と同じ語の指定で合わせる:

```bash
history-rewrite sync-worktrees --workdir "$D" "${WORDS[@]}"
```

ツールは `worktree-state` の行ごとに、変更の無い worktree を新しい HEAD に合わせ（detached HEAD は `commit-map` で
移す。追跡していないファイルは残る）、変更のある worktree は変更を残したまま、書き換えたファイルに同じ変換を当てて
番号を付け直す（何をどの順に行うかと出力の形は、ツールの先頭の説明）。

- `KEPT:` の worktree: `git -C <パス> diff HEAD` が、控えのパッチと同じ変更に語と番号の変換を当てたものだけに
  なっていることを確かめる。書き換えで変わらなかったファイルへの変更は、番号だけを付け直し、語は直さない
- `LEFT:` の worktree: ツールは何も変えていない。理由を見て、合わせ方をユーザーと決める（対応表に無いコミットなら、
  1 でブランチを付けて 1 からやり直す）
- `ERROR:` の worktree（`commit-refs remap` が止まった）: 語は直したが番号は付け直していない。理由を直し、その
  worktree で `commit-refs remap "$D/commit-map"` を実行する

## 6. 書き換えの外に残るもの

- `refs/remotes/`、`refs/stash`、notes は移さない。リモートに書き換えた履歴を送るには force push が要るので、
  ユーザーが決める（GitHub のバックアップのリモートも同じ）
- DVC: `dvc.lock` の出力の md5 は出力の中身で決まるので、語の書き換えでは変わらない。deps に書き換えたファイル
  （文書、コメントを直したスクリプト）があるステージは `dvc status` で changed になる。過去のコミットの `dvc.lock` は、
  書き換える前の deps の md5 のまま。そのリポジトリの DVC の規則（xlc の `/dvc-relock`）は、走らせなかったステージの
  changed を「そのままでよい」とするので、規則に任せると何も起きない。2026-10-21 の xlc の書き換えでは、issue も
  開かれず、下流を流すときに上流が再実行される費用（p2 で 1 時間 44 分）も見積もられなかった。書き換えのあとは次を行う:
  - (a) 書き換えが原因で changed になったステージを一覧にする。`dvc status` で deps が changed のステージ（`always_changed` の
    ステージと、`changed outs` だけのステージは除く）のうち、書き換えの直前の HEAD で changed でなかったものを選ぶ。控えの
    original.git は bare なので `dvc status` は走らない。`git clone "$D/original.git" <一時のパス>` で作業ツリーを作り、
    そのリポジトリの DVC の規則に従って出力を戻して（xlc なら `references` の symlink と `dvc checkout`、`build`）
    そこで `dvc status` を実行する
  - (b) lock を付け直さないなら、対象のリポジトリに issue を開き、(a) の一覧と、下流のステージを流すときに上流が
    再実行される費用の見積もり（`dvc.yaml` のステージの依存と、記録された所要時間から）を書く
  - (c) 付け直すなら、deps の差が出力を変えないことを、書き換えの前後の diff でステージごとに確かめる（文書の文言と
    コメントだけの差なら可。文字列リテラル、ログの書式、ステージが読む語の差は出力を変えうるので不可で、(b) にする）。
    `dvc commit -f <ステージ>` を実行する（`-f` が無いと、deps が変わったステージは確認を求めて止まる。出力は作り直さず、
    今の deps と outs の md5 を lock に書く）。lock は、そのリポジトリの DVC の規則の検査（xlc なら
    `scripts/check-dvc-lock.sh`）を通してコミットする。コードの変更に `dvc commit` を使うことは、記録した deps で
    作っていない出力を lock が指すことになるので、しない
- ほかのリポジトリやメモリの中の、このリポジトリの番号は、そのリポジトリで `commit-refs remap "$D/commit-map"` で
  付け直す

## 7. 古いオブジェクトの掃除を issue にする

書き換える前のオブジェクトは reflog から参照され、リポジトリに残る。`git reflog expire --expire=now --all &&
git gc --prune=now` と作業ディレクトリ（ミラーの控え）の削除は取り消せないので、ここでは行わず、対象のリポジトリの
issue にしてユーザーが決める。issue には、書き換えた日、直した語（辞書と追加の規則）、作業ディレクトリのパス、
控えの場所、完了の条件（ユーザーが書き換えた後の履歴で困らないと判断したら、上のコマンドを行う）を書く。
