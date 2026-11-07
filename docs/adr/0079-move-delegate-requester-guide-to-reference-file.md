---
status: accepted
date: 2026-11-08
requires: [ADR-0027]
supersedes: []
superseded-by:
issues: ["#645"]
---

# ADR-0079: /delegate の頼む側の書き方を、担当が読まない references/requesting.md に移す

## 背景

/delegate の SKILL.md は、担当（サブエージェント）の手順と、親が依頼を書くときの「頼む側（親のセッション）の書き方」の
節を 1 つのファイルに持っていた（ADR-0027）。担当は依頼のたびに SKILL.md の全体を文脈に読み込むが、頼む側の節は親しか
使わない。#646 で経緯と例を消して 28,228 文字から 22,071 文字にしたが、半分には届かず、マージの本文に「半分にするには
「頼む側の書き方」を別ファイルに分ける構造の変更が要る（#645）」と残した。main 12c1d104 の SKILL.md は 22,079 文字で、
頼む側の節はそのうち 7,147 文字。

## 検討した案

- 案 A: 今のまま 1 つのファイルに置く。担当は頼む側の 7,147 文字を毎回読む
- 案 B: 頼む側を別のスキル（例: `/delegate-request`）にする。親は Skill で呼ぶ。スキルの一覧の説明がどのセッションの文脈にも
  1 行ずつ入り、受ける側の手順 1 と頼む側の項が互いを指すとき、スキルの名前をまたぐ
- 案 C: 頼む側の節を、同じスキルのディレクトリの `references/requesting.md` に移し、SKILL.md には頼む側がそれを Read で読む
  1 行だけを残す。language-checks の `references/` と同じ置き方で、claude-sync がスキルのディレクトリごと `~/.claude` に入れる

## 決定と理由

案 C。担当の読む量が頼む側の分だけ減り、親は SKILL.md の 1 行から同じ文を読める。案 B と違い、スキルの一覧を増やさず、
受ける側と頼む側は同じディレクトリの中で互いを指せる。

- 文は移すだけで字句を変えない（見出しの段だけ、ファイルの先頭なので `#` にする）。移した節を指す文（受ける側の手順 1 の
  「前提」の項、/git-merge の Agent の `description` の項、hook delegate-request-fields の拒否の文言、ADR の確認）は新しい
  置き場を指す形にする
- 受ける側の引用の確かめ方の `jq` のプログラムは `.claude/scripts/quote-in-transcript.sh` に移し、受ける側と頼む側はそれを
  指す（ADR-0065 の「## 変更」）

### 帰結

- /delegate の SKILL.md は 22,079 文字から 14,138 文字になる（このブランチで戻した #645 の 5 件のうち /delegate の 2 件と、
  kept-sentences.py を指す手順 6 の文を含む）。`references/requesting.md` は 7,215 文字
- 親は委任のたびに `references/requesting.md` を Read で読む 1 回の呼び出しが増える。SKILL.md の 1 行と、hook
  delegate-request-fields の拒否の文言がそこを指す
- 頼む側の節を指していた文が残ると、親は無い節を探す。指す文は `grep -rn 頼む側` で探せる

### 確認

- `.claude/skills/delegate/SKILL.md` に頼む側の節が無く、冒頭に `references/requesting.md` を Read で読む 1 行がある
- `.claude/scripts/kept-sentences.py --old <(git show 12c1d104:.claude/skills/delegate/SKILL.md) --new
  .claude/skills/delegate/SKILL.md .claude/skills/delegate/references/requesting.md` が、指す先を替えた文と、このブランチで
  足した文と戻した文だけを出す
- claude-hooks の `missing_fields_reason_points_to_the_requester_guide` が、拒否の文言が `references/requesting.md` を指すことを
  確かめる
