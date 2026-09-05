---
name: gh-pr-review
description: PRレビューコメントを確認し対応。コード修正、返信を実行。
# model 未指定: コード修正を伴うためセッションモデルを継承する
argument-hint: [PR番号 | コメントURL] [--unresolved]
allowed-tools:
  - Bash(git *)
  - Bash(gh *)
  - Bash(~/.claude/skills/gh-pr-review/scripts/*)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh)
  - Bash(~/.claude/scripts/gh-actions-diagnose.sh*)
  - Read
  - Edit
  - Glob
  - Grep
  - WebSearch
  - WebFetch
  - Agent
---

# GitHub PR Review Command

PRについたレビューコメントを確認し、対応する。

## ヘルパースクリプト

このスキルは `scripts/` ディレクトリのスクリプトを使用:

- `get-pr-info.sh [pr_number | URL]` - PR情報を取得。コメント URL を渡すと `comment_id` も返す
- `get-review-comments.sh <pr_number> [--unresolved]` - レビューコメントを取得
  （GraphQL `reviewThreads` から。各コメントに `thread_id` / `is_resolved` / `is_outdated` /
  `user_type`（Bot / User）を含む）
- `get-latest-review.sh <pr_number>` - 最新の Copilot レビューの要約を取得
  （周回数 ROUND、インライン指摘数、Suppressed comments の本文。Step 6 の判定に使う）
- `reply-to-comment.sh <pr_number> <comment_id> <body>` - コメントに返信
- `resolve-thread.sh <pr_number> <comment_id> [--allow-human]` - コメントを含むスレッドを resolve
  （GraphQL `resolveReviewThread`。人間が起こしたスレッドは既定で拒否する。
  生の `gh api graphql` による resolve は `guard-gh-api.sh` hook が deny する）
- `decide-next.sh <pr_number> [--max-rounds N]` - 周回数と直近の Copilot 応答から
  次の行動（VERDICT）を判定。Step 6 の分岐はこの出力に従う
- `request-rereview.sh <pr_number> [commit_hash ...]` - @copilot に再レビューを依頼し、
  依頼コメントの時刻を基準に応答を待つ（約10分。バックグラウンドで実行）

判断はモデルが行い、取得・集計・状態判定はスクリプトに寄せる。手順中で
「数える」「比べる」「探す」が必要な箇所は、記憶に頼らずスクリプトの出力を使う。

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git status --short`
!`git branch -vv | head -5`

## Step 1: 引数の解析

### パターン A: コメントURL が指定された場合

URL形式: `https://github.com/{owner}/{repo}/pull/{pr_number}#discussion_r{comment_id}`

```bash
~/.claude/skills/gh-pr-review/scripts/get-pr-info.sh "{URL}"
```

出力の `number` が PR 番号、`comment_id` が対象コメント → そのコメントだけに対応する
（Step 2 で取得した一覧から `id` が一致するものを使う）

### パターン B: PR番号が指定された場合

→ そのPRの全レビューコメントを取得

### パターン C: 引数なし

→ 現在のブランチに関連するPRを特定

```bash
~/.claude/skills/gh-pr-review/scripts/get-pr-info.sh
```

---

## Step 2: レビューコメントの取得

```bash
# 未解決コメントのみ取得（--unresolved オプション時）
~/.claude/skills/gh-pr-review/scripts/get-review-comments.sh {pr_number} --unresolved

# 全コメント取得
~/.claude/skills/gh-pr-review/scripts/get-review-comments.sh {pr_number}

# 最新 Copilot レビューの要約（周回数・インライン指摘数・Suppressed comments）
~/.claude/skills/gh-pr-review/scripts/get-latest-review.sh {pr_number}
```

**Suppressed comments** は Copilot が低確度と判断した指摘で、インラインスレッドに
ならずレビュー本文にだけ現れる（`get-review-comments.sh` では見えない）。
通常のコメントと同様に Step 3 で分類し、Step 4 で対応する。ただしスレッドが
ないため個別返信はできず、Step 6 の再レビュー依頼の対象にもならない。

---

## Step 3: コメントの優先度分類

取得したコメントを以下の優先度で分類:

### 🔴 Critical（必須対応）

- バグ指摘
- セキュリティ問題
- ビルド/テスト失敗の原因
- "must", "required", "blocking" などのキーワード

### 🟡 Warning（対応推奨）

- コード品質の問題
- パフォーマンス懸念
- "should", "consider", "recommend" などのキーワード

### 🟢 Suggestion（任意対応）

- スタイル提案
- リファクタリング案
- "nit", "optional", "nice to have" などのキーワード

### ℹ️ Question（回答のみ）

- 設計意図の質問
- 確認事項
- "?" を含む、"why", "how" で始まる

---

## Step 4: 対応の実施

### Atomicコミットの原則

**1コメント = 1コミット** を徹底する。各レビューコメントに対して個別にコミットを作成することで:

- 変更の追跡が容易になる
- 問題発生時のrevertが簡単
- レビュアーが対応を確認しやすい

### 優先順位: Critical → Warning → Suggestion → Question

### 4.1 各コメントへの対応サイクル

**以下のサイクルをコメントごとに繰り返す:**

#### (a) コード修正

1. 対象ファイルを読み取り（Read）
2. 関連コードを調査（Grep, Glob, または Agent ツールで Explore）
3. 外部情報が必要なら調査（WebSearch, WebFetch）
4. 修正を実施（Edit）

#### (b) ビルド/テスト確認

プロジェクトの言語を検出してフォーマット → リント → テストを実行する
（プロジェクトの CLAUDE.md にチェックコマンドが明記されていればそちらを優先。詳細は language-checks スキル）:

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh
```

`FAILED:` で止まったら `FIX:` の自動修正コマンドを使うか手で直し、`ALL_OK` になるまで繰り返す。

#### (c) Atomicコミット

レビュー対応であることとコメントIDを明記:

```bash
git add {修正ファイル}
git commit -m "$(cat <<'EOF'
fix: {コメントで指摘された内容を簡潔に}

Refs: {コメントURL または #discussion_r{comment_id}}
EOF
)"
```

#### (d) 次のコメントへ

全コメントの対応が完了するまで (a)〜(c) を繰り返す。

### 4.2 全対応完了後にプッシュ

```bash
git push
```

---

## Step 5: レビューコメントへの返信

### コメントへの返信

```bash
~/.claude/skills/gh-pr-review/scripts/reply-to-comment.sh \
  {pr_number} {comment_id} "{返信内容}"
```

### 一般コメントとして返信（スレッドに属さない場合）

```bash
gh pr comment {pr_number} --body "{返信内容}"
```

### 対応済みスレッドの resolve

スレッドを閉じるのはレビュアーの権限。`user_type` で分ける:

- **bot（`Bot`。Copilot 等）のスレッド**: 返信したら resolve する（リポジトリによっては
  スレッド解決が bot の再レビュー自動化のトリガーになる）
- **人間（`User`）のスレッド**: 返信だけ行い、resolve はレビュアーに委ねる。修正済みでも
  閉じない（相手が確認して閉じる）。スクリプトは人間のスレッドを既定で拒否する。
  `--allow-human` はユーザーから明示的に「resolve してよい」と言われたときだけ付ける

```bash
~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh {pr_number} {comment_id}
```

人間のスレッドが未解決のままだと `pre-merge-check.sh` がマージを止める。それは正しい状態なので、
resolve で回避せず、レビュアーの確認を待つ（または Step 8 で「レビュアーの確認待ち」として報告する）。

### 返信テンプレート

**修正完了時:**

```text
Fixed in {commit_hash}.
```

**対応不要と判断した場合:**

```text
Thank you for the suggestion.
I've decided to keep the current approach because {理由}.
```

**質問への回答:**

```text
{回答内容}
```

---

## Step 6: 再レビューの依頼と待機

### 6.1 再レビューを依頼するかの判定

再レビューは毎周必ず新しい指摘を生みうるため、「指摘ゼロになるまで」を
終了条件にすると収束しない。**周回数の上限は 5 周**（周回数 = 1 + 再レビュー
依頼の回数。Copilot がコメントだけで応答した周も数える）。

判定はスクリプトに任せる:

```bash
~/.claude/skills/gh-pr-review/scripts/decide-next.sh {pr_number}
```

`VERDICT` に従って分岐する:

| VERDICT                | 意味                          | 対応                     | 再レビュー依頼         |
| ---------------------- | ----------------------------- | ------------------------ | ---------------------- |
| `REREVIEW`             | インライン指摘あり、ROUND < 5 | 対応・push               | **する** → 6.2 へ      |
| `STOP_LIMIT`           | インライン指摘あり、ROUND = 5 | 対応・push               | **しない** → Step 7 へ |
| `STOP_SUPPRESSED_ONLY` | Suppressed comments のみ      | 対応・push               | **しない** → Step 7 へ |
| `STOP_CLEAN`           | 指摘なし                      | —                        | しない → Step 7 へ     |
| `COMMENT_ONLY`         | Copilot がコメントだけで応答  | 本文を読んで判定（下記） | 本文次第               |
| `WAITING`              | 依頼後の応答が未着            | gh-wait-review.sh で待つ | —                      |

`COMMENT_ONLY` はスクリプトでは判定できない唯一の分岐。出力された本文を読み、
対応確認のみ（「対応を確認しました」「追加修正は不要」等）なら `STOP_CLEAN`
相当、新しい指摘を含むなら `ROUND` を見て `REREVIEW` / `STOP_LIMIT` 相当として
扱う。

Suppressed comments は Copilot 自身が低確度と判断したものなので、対応は
するが再レビューで確認は求めない。上限到達時は、5 周目で見送った指摘を
Step 8 の完了報告に列挙してユーザーの判断に委ねる（自分で 6 周目を始めない）。

### 6.2 再レビューの依頼

pushしても再レビューは自動では走らないことがある。対応をプッシュしたら
@copilot に再レビューを依頼し、応答を待つ:

```bash
# 依頼コメントを投稿し、その created_at を基準に応答を待つ（漸増バックオフで約10分）
# フォアグラウンドの最大タイムアウトを超えるため、シェルの & ではなく
# Bashツールの run_in_background=true で実行する（完了時に通知される）
~/.claude/skills/gh-pr-review/scripts/request-rereview.sh {pr_number} {commit_hash...}
```

依頼の投稿と待機を分けると、その間に届いた応答を取りこぼして約10分
タイムアウトする競合がある。必ずこのスクリプトで一体化して実行する。

注意:

- `requested_reviewers` API に `copilot-pull-request-reviewer[bot]` を渡す
  方法は、bot が collaborator ではないリポジトリでは 422、自分に push 権限が
  ないリポジトリ（fork からの upstream PR）では 404 で失敗する。
  @copilot メンションコメントを使うこと
- Copilot は正式なレビュー提出ではなく **PRコメントだけで応答する**ことが
  ある（「対応を確認しました」等）。gh-wait-review.sh は両方を検出し、
  コメント検出時は `NOTE:` 行を付けるので、内容を読んで対応要否を判断する
- レビューボットによっては（例: Greptile）**指摘ゼロのとき何も投稿せず**
  check-run だけ成功させるため、gh-wait-review.sh はクリーンな結果でも
  タイムアウトする。タイムアウトを「トリガー失敗」と誤読せず、head SHA の
  check-run と未解決スレッド数で判断する

応答が届いたら `decide-next.sh` を再実行し、6.1 の表に従う。新しいレビュー
提出なら Step 2 に戻ってコメントを取得・対応する。`decide-next.sh` は直近の
依頼より後に届いた応答だけを見るので、前回レビューの指摘を今周のものと
取り違えることはない。

---

## Step 7: CI確認

```bash
~/.claude/scripts/gh-actions-diagnose.sh {pr_number}
```

`CAUSE:` に従う（分類の意味は gh-actions-check スキル）。特に:

- `PERMISSION`: ワークフローの `permissions:` に必要な権限を最小限で宣言する。リポジトリ設定
  `default_workflow_permissions` を write に緩めるのは回避策であり使わない
- `TRANSIENT_API` / `COPILOT_INTERNAL`: 再実行は `NEXT:` が提案する 1 回だけ。`ATTEMPT: 2` 以上で
  同じ失敗なら再実行せず、Step 8 の完了報告に載せてユーザー判断に委ねる
- `EXTERNAL`: 外部 CI のログを `EXTERNAL_CHECK:` の URL で確認する。`gh run rerun` では直らない
- `CODE`: 原因を修正し、レビュー対応と同じくコミットして push する

---

## Step 8: 完了報告

```text
✅ レビュー対応が完了しました

PR: {url}
コミット: {hash}

レビュー周回: {ROUND}/5
終了理由: {指摘なし | Suppressed comments のみ（再レビュー未依頼） | 周回上限到達}

対応サマリー:
- 🔴 Critical: {n}件 対応済み
- 🟡 Warning: {n}件 対応済み
- 🟢 Suggestion: {n}件 対応済み/{m}件 見送り
- ℹ️ Question: {n}件 回答済み
- 未確認の対応: {周回上限到達時、最終周で対応・見送りした指摘の一覧。該当なしなら省略}

次のステップ:
- マージする場合 → /gh-pr-merge
- 状態を確認する場合 → /git-info
```

---

## 注意事項

- **Atomicコミット**: 1コメント = 1コミットを徹底
- Force push は避け、追加コミットで対応（レビュー履歴を保持）
- Critical は必ず対応、Suggestion は見送り可（理由を返信）
- 再レビューは最大 5 周。Suppressed comments のみの周は対応して終了し、再レビューを依頼しない
- レビュアーの意図が不明な場合は、修正前に確認コメントを投稿
