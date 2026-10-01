---
name: git-push
description: ローカルコミットをリモートにプッシュ。main/masterへの直接プッシュは禁止。
model: haiku
argument-hint: [--force]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git push*)
---

# Git Push Command

ローカルコミットをリモートリポジトリにプッシュする。

## 現在の状態

!`git status --short`
!`git branch -vv`

## main / master では push しない

現在のブランチが `main` / `master` なら push せず、/git-cherry-pick で feature ブランチに
移してから /gh-pr-create へ進むよう案内する。hook に deny されたときの
`ALLOW_PROTECTED_PUSH=1` は、ユーザーの明示的な指示があるときだけ付ける。

## プッシュ対象の確認

```bash
git log @{upstream}..HEAD --oneline 2>/dev/null || echo "上流ブランチ未設定（push で自動設定される）"
```

## プッシュ実行

上流ブランチは `push.autoSetupRemote` により初回 push で設定される:

```bash
git push
```

### --force オプションが指定された場合

`--force` ではなく `--force-with-lease` を使う:

```bash
git push --force-with-lease
```

## 完了確認

```bash
git branch -vv
git log --oneline -3
```

## 次のステップ

```text
✅ プッシュしました。

次のステップ:
- PRを作成する場合 → /gh-pr-create
- 状態を確認する場合 → /git-info
```
