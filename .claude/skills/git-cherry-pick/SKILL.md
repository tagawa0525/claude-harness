---
name: git-cherry-pick
description: ブランチ間でコミットを移動（元ブランチからの除去含む）。mainで作業してしまった変更を移す場合などに。
# model 未指定: 履歴書き換えとコンフリクト解決の判断が必要なためセッションモデルを継承する
argument-hint: <commit> [--to <branch>]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git show*)
  - Bash(git switch*)
  - Bash(git fetch*)
  - Bash(git checkout*)
  - Bash(git cherry-pick*)
  - Bash(git rebase*)
  - Bash(git reset*)
  - Bash(git stash*)
---

# Git Cherry-Pick Command

ブランチ間でコミットを移動する（元ブランチからの除去を含む）。

## 現在の状態

!`git status --short`
!`git branch -vv`
!`git log --oneline -15`

## 対象と移動先

- 移動するコミット: $ARGUMENTS のハッシュ。なければ履歴を分析して候補を提案する
- 移動先: `--to` のブランチ。なければ新規ブランチを作るか確認し、名前を提案する

## main で作業してしまったコミットを移す

`reset --hard` は未コミットの変更も消すので、先に `git status` で確認して必要なら stash する。

**すべて**移すなら cherry-pick は不要（新ブランチが既に含む）:

```bash
git fetch origin
git switch -c [new-branch]
git switch main
git reset --hard origin/main
```

**一部だけ**移すなら、全コミットの行き先（各ブランチ・破棄）を先に決める:

```bash
git fetch origin
git branch [backup] main             # 確認後に削除
git switch -c [branch] origin/main   # 行き先ごとに
git cherry-pick [commit...]
git switch main
git reset --hard origin/main
```

## feature ブランチ間で移す

```bash
git switch [移動先]            # 新規なら git switch -c [移動先] main
git cherry-pick [commit]
git switch [移動元]
git rebase --onto [commit]^ [commit]   # 移動元から対象コミットだけを除く
```

## 注意

- 移動元が push 済みなら `/git-push --force` が要ることを伝え、確認を得てから実行する
- コンフリクトしたら内容と解決案を示し、`--continue` か `--abort` を案内する
