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

`history-rewrite`（`modules/home/parts/history-rewrite/`、ADR-0045）で、対象のリポジトリ（$ARGUMENTS）のすべてのブランチと
タグの語を辞書で直し、番号を `commit-refs rewrite-history`（ADR-0013）で付け直す。ツールは対象のリポジトリに書き込まず、
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
  その語が直す対象の造語なら除かない（ADR-0045）

## 3. 書き換えて照合する

作業ディレクトリは、対象のリポジトリの外の、空のディレクトリにする。中のミラー（`original.git`）が書き換える前の
控えになるので、古いオブジェクトを消すと決めるまで残す。

```bash
history-rewrite run --source <リポジトリ> --workdir <作業ディレクトリ> [--extra-rules <ファイル>] [--exclude <パス>]...
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

各 worktree の HEAD と、未コミットの変更の有無を、ref を移す前に控える（移した後は、新しい HEAD との差になる）。
変更の控えのパッチも残す:

```bash
# パスは空白を含みうるので、行の残り全体を取り、列はタブで区切る。控えのパッチは行の番号で名付ける
git -C "$R" worktree list --porcelain | sed -n 's/^worktree //p' >"$D/worktrees"
k=0
while IFS= read -r w; do
  k=$((k + 1))
  git -C "$w" diff HEAD >"$D/worktree-$k.patch"
  printf '%s\t%s\t%s\t%s\n' "$w" "$(git -C "$w" rev-parse HEAD)" \
    "$(git -C "$w" status --porcelain --untracked-files=no | wc -l)" "$D/worktree-$k.patch"
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
git -C "$R" update-ref --stdin <"$D/transaction"
```

`git fetch` で `refs/heads/` に直接取り込まないのは、worktree にチェックアウト中のブランチを fetch が拒むため
（`--update-head-ok` で外すと、条件つきの更新にならない）。

## 5. worktree を合わせる

`update-ref` はチェックアウト中のブランチも移すので、各 worktree の index と作業ツリーは元の番号のままになる。
`worktree-state` の行ごとに合わせる（`while IFS=$'\t' read -r w old n patch` で読む。`n` は未コミットの変更の数、`patch` は控えのパッチ）。

- detached HEAD の worktree: 先に HEAD を移す（`commit-map` は元の番号から最後の番号への対応表）

  ```bash
  new=$(awk -v o="$old" '$1 == o {print $2}' "$D/commit-map")
  [ -n "$new" ] || echo "ERROR: $old は書き換えに入っていない（1 で確かめる）"
  git -C "$w" update-ref --no-deref HEAD "$new" "$old"
  ```

- 変更の無かった worktree: `git -C "$w" reset -q --hard`。書き換えはパスを足さないので、追跡していないファイル
  （`references` の symlink など）は残る
- 変更のあった worktree: 作業ツリーを壊さない。index だけを新しい HEAD に合わせ、書き換えで変わったファイルを
  ファイルごとに合わせる

  ```bash
  # ステージしていたパスを控える（mixed reset で外れる。足したばかりのファイルは追跡されなくなる）
  git -C "$w" diff --cached -z --name-only >"$D/staged-$k"
  git -C "$w" reset -q   # mixed。ステージしていた変更は、作業ツリーに残ったまま外れる
  # -z で受ける（日本語や空白を含むパスを、引用符つきの 8 進や語の区切りにしない）
  git -C "$w" diff -z --name-only "$old" HEAD | while IFS= read -r -d '' f; do
    if [ ! -e "$w/$f" ] && [ ! -L "$w/$f" ]; then
      continue   # 未コミットで消したファイルは、消したまま残す
    elif git -C "$w" diff --quiet "$old" -- "$f"; then
      git -C "$w" checkout -- "$f"   # 手を付けていないファイルは、新しい中身にする
    else
      (cd "$w" && history-rewrite transform [語の指定] "$f")   # 変えたファイルは、同じ変換を当てる
    fi
  done
  (cd "$w" && commit-refs remap "$D/commit-map")   # 未コミットの文書の古い番号を付け直す
  # ステージしていたパスを、残っているものだけ戻す（ファイルの全体をステージするので、部分のステージは戻らない）
  xargs -0 -r -I{} sh -c 'test -e "$1/$2" && git -C "$1" add -- "$2"' _ "$w" {} <"$D/staged-$k"
  ```

  `git -C "$w" diff HEAD` が、控えのパッチ（`$patch`）と同じ変更に語と番号の変換を当てたものだけに
  なっていることを確かめる（`k` は `worktree-state` の行の番号）

## 6. 書き換えの外に残るもの

- `refs/remotes/`、`refs/stash`、notes は移さない。リモートに書き換えた履歴を送るには force push が要るので、
  ユーザーが決める（GitHub のバックアップのリモートも同じ）
- DVC: `dvc.lock` の出力の md5 は出力の中身で決まるので、語の書き換えでは変わらない。deps に書き換えたファイル
  （文書、コメントを直したスクリプト）がある段階は `dvc status` で changed になる。そのリポジトリの DVC の規則
  （xlc なら `/dvc-relock`）に従って lock を直す。過去のコミットの `dvc.lock` は、書き換える前の deps の md5 のまま
- ほかのリポジトリやメモリの中の、このリポジトリの番号は、そのリポジトリで `commit-refs remap "$D/commit-map"` で
  付け直す

## 7. 古いオブジェクトの掃除を issue にする

書き換える前のオブジェクトは reflog から参照され、リポジトリに残る。`git reflog expire --expire=now --all &&
git gc --prune=now` と作業ディレクトリ（ミラーの控え）の削除は取り消せないので、ここでは行わず、対象のリポジトリの
issue にしてユーザーが決める。issue には、書き換えた日、直した語（辞書と追加の規則）、作業ディレクトリのパス、
控えの場所、完了の条件（ユーザーが書き換えた後の履歴で困らないと判断したら、上のコマンドを行う）を書く。
