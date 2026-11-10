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

案 C に加えて、受ける側（担当）の手順も同じディレクトリの `references/working.md` に移し、SKILL.md はフロントマターと、
受ける側は `references/working.md` を、頼む側は `references/requesting.md` を Read で読む案内だけにする。担当が読む手順から
頼む側の分が減り、親が Skill で /delegate を呼んでも、担当の手順の全体は親の文脈に入らない。親の文脈はセッションが続く限り
毎回読み直されるので、担当の文脈より高くつき、担当は Read が 1 回増えるだけで済む（会話が圧縮されたら、SKILL.md の案内の
とおりもう 1 回読み直す）。案 B と違い、スキルの一覧を増やさず、
受ける側と頼む側は同じディレクトリの中で互いを指せる。

- 文は移すだけで字句を変えない（見出しの段だけ、ファイルの先頭なので `#` にする。受ける側の手順の先頭の見出しは、置き場に
  合わせて替える）。移した節を指す文（受ける側の手順 1 の「前提」の項、/git-merge の Agent の `description` の項、hook
  delegate-request-fields の拒否の文言、ADR の確認）は新しい置き場を指す形にする。ほかのスキル、CLAUDE.md、hook が
  「/delegate の手順 7」のようにスキルの名前と項で指す文は、SKILL.md の案内からたどれるので変えない

### 帰結

- main 52b80a9d の /delegate の SKILL.md（14,576 文字）は、フロントマターと案内の 1,267 文字の SKILL.md と、13,678 文字の
  `references/working.md` に分かれる。`references/requesting.md` は 7,278 文字
- 担当は、作業の前に `references/working.md` を Read で読む 1 回の呼び出しが増える。Claude Code は圧縮の後に呼んだスキルの
  本文（SKILL.md の案内）を付け直すが、Read の結果は残るとは限らないので、圧縮の後は案内のとおりもう 1 回読む。前提の出典の種類を確かめるときだけ、
  受ける側の手順 1 が絶対パスで指す `references/requesting.md` の「前提」の項を読む（出典の規則は 1 か所に置く。ADR-0060）
- 親は委任のたびに `references/requesting.md` を Read で読む 1 回の呼び出しが増える。SKILL.md の案内と、hook
  delegate-request-fields の拒否の文言がそこを指す。親が Skill で /delegate を呼んでも、文脈に入るのは案内だけになる
- 頼む側の節を指していた文が残ると、親は無い節を探す。指す文は `grep -rn 頼む側` で探せる

### 確認

- `.claude/skills/delegate/SKILL.md` に頼む側の節も受ける側の手順も無く、受ける側が `references/working.md` を、頼む側が
  `references/requesting.md` を Read で読む案内がある
- `.claude/scripts/kept-sentences.py --old <(git show 52b80a9d:.claude/skills/delegate/SKILL.md) --new
  .claude/skills/delegate/SKILL.md .claude/skills/delegate/references/working.md` が、SKILL.md の description と案内として足した文と、
  `references/working.md` の先頭の見出しだけを出す
- claude-hooks の `missing_fields_reason_points_to_the_requester_guide` が、拒否の文言が `references/requesting.md` を指すことを
  確かめる

## 変更

- 2026-11-11: 受ける側の手順も `references/working.md` に移し、SKILL.md を案内だけにした（決定と理由の初めの段落と
  1 つ目の項）。前は「案 C。担当が毎回読む SKILL.md から頼む側の分が減り、親は SKILL.md の 1 行から同じ文を読める。」と
  「文は移すだけで字句を変えない（見出しの段だけ、ファイルの先頭なので `#` にする）。」。親のセッションは初め、担当の側に
  得るものが無いとして受ける側の手順を移さない案を採っていた。ユーザーは、1 つのスキルにまとめる理由は認めたうえで、頼む側を
  references に分けているなら受ける側の手順も同じ置き方にできるのではないかと提案し、続けて、親の文脈は担当の文脈より
  貴重だと指摘した（ユーザー 2026-11-11）。これを受けて移した。
  2026-11-11 に `~/.claude/projects/*/*.jsonl` の親の tool_use を数えると、11 月 1 日以降に /delegate の依頼を出した親の
  セッションは 2 件、依頼は 209 回で、親が Skill で delegate を呼んだのは 1 回、`references/requesting.md` を Read したのは 5 回。
  別のスキルにする案 B を採らない理由は変わらない
- 2026-11-11: 決定と理由の、引用の確かめ方の項を消した。前は「受ける側の引用の確かめ方の `jq` のプログラムは
  `.claude/scripts/quote-in-transcript.sh` に移し、受ける側と頼む側はそれを指す（ADR-0065 の「## 変更」）」。ユーザーの言葉を
  括らずに要約で書く形にして（ADR-0081）、引用を確かめる手順とその script が無くなった
