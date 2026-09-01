---
name: git-branch
description: featureブランチの作成・リネーム。変更内容から適切なブランチ名を自動生成。
# 変更内容の分析とブランチ名生成の品質を優先
model: sonnet
allowed-tools:
  - Bash(git *)
  - Bash(~/.claude/scripts/rename-plan.sh*)
  - Bash(~/.claude/scripts/rename-branch.sh*)
  - Read
  - Glob
  - AskUserQuestion
---

# Git Branch Command

変更内容から適切なブランチ名を自動生成し、ブランチの作成またはリネームを行う。
このスキルの本体はブランチ名を考えること。作成・リネーム・計画書のリネームはスクリプトに任せる。

## 現在の状態

!`git branch -vv`
!`git status --short`
!`git log --oneline -5`
!`git fetch -q; git log --oneline HEAD..origin/main 2>/dev/null | head -3`
!`~/.claude/scripts/rename-plan.sh --list`

## 動作モードの判定

現在のブランチによって動作が変わる:

- **mainブランチ** → 新規featureブランチを作成
- **featureブランチ** → 現在のブランチをリネーム

## ブランチ名の自動生成

**常にLLMがブランチ名を生成する。** 引数は受け付けない。

### Step 1: 計画書の把握

上の `rename-plan.sh --list` の出力（連番のない計画書）があれば、その内容を読み、
タイトル（`# ...` 行）から意図を把握する。計画書の情報はブランチ名生成の最優先ソースとして使用する。

### Step 2: 変更内容の分析

計画書がない場合、以下を確認してブランチ名を決定:

- mainブランチの場合:
  1. ステージされた変更: `git diff --cached --name-only`
  2. 未ステージの変更: `git diff --name-only`
  3. 変更ファイルの内容を読んで意図を把握
- featureブランチの場合:
  1. mainとの差分: `git log --oneline main..HEAD`
  2. 未コミットの変更: `git diff --name-only`
  3. コミットメッセージとファイルの内容から全体の意図を把握

### Step 3: 命名規則に従い候補を生成

- `feat/xxx` - 新機能
- `fix/xxx` - バグ修正
- `refactor/xxx` - リファクタリング
- `docs/xxx` - ドキュメント
- `chore/xxx` - その他

### Step 4: AskUserQuestion で候補を提示

2-3個の候補を生成し、ユーザーに選択させる。
「Other」で自由入力も可能。

## mainブランチの場合: ブランチ作成

### 起点の確認

ローカルmainが `origin/main` より遅れている場合（上記のログ出力がある場合）は警告:

```text
⚠️ ローカルmainがorigin/mainより遅れています。

1. pullしてから分岐 (推奨)
2. origin/mainから直接分岐
3. このまま続行
```

### ブランチ作成コマンド

```bash
# 通常（ローカルHEADから分岐）
git switch -c [branch-name]

# origin/mainから分岐する場合
git switch -c [branch-name] origin/main
```

## featureブランチの場合: リネーム

```bash
~/.claude/scripts/rename-branch.sh [new-name]
```

- exit 2（`STOP:` 行）: リモートに同名ブランチがある。リモートも更新されることをユーザーに
  伝えて承認を得てから `--remote` を付けて再実行する
- open PR の head ブランチはスクリプトが拒否する（リモート削除で PR が閉じるため）。
  その場合はリネームせず、PR をマージ/クローズしてから行うよう案内する

## 計画書のリネーム

ブランチ作成・リネーム後、連番のない計画書があれば連番付きにリネームする:

```bash
~/.claude/scripts/rename-plan.sh [branch-name]
```

連番・名前（`NNN_<ブランチ名から type/ を除き - を _ に>.md`）・`git mv` とコミットはスクリプトが行う。
`COMMITTED: no` の場合は計画書が git 追跡外（`docs/plans/*.md` が gitignore されている等）なので、
リネームだけ行われている。複数の候補があってエラーになったら、対象を確認して `--file` で指定する。

## 完了確認

```bash
git branch -vv
```

```text
✅ ブランチ「[branch-name]」を[作成|リネーム]しました。
[計画書リネーム時: 📋 計画書を [NNN]_[name].md にリネームしました。]

次のステップ:
- コードを編集 → /git-commit
- プッシュ → /git-push
- PR作成 → /gh-pr-create
```
