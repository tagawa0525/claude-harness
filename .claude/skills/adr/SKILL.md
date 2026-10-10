---
name: adr
description: ADR（docs/adr/）を書く・直すときに使う。MADR 4.0.0 を基準に、前付け、見出し、運用を調整した形。
allowed-tools:
  - Bash(git config*)
  - Bash(git ls-files*)
---

# ADR

何をなぜ決めたかを、1 件 1 決定で残す。形は [MADR](https://adr.github.io/madr/) 4.0.0 を基準にし、ほかの仕組みの
良いところ（xlc、PEP、KEP、ADR の事例集）を取り入れて調整している（ADR-0005）。

## このリポジトリの扱い

!`git config --default unset --get claude-hooks.own-project`

`false` なら他人のプロジェクト。そのプロジェクトの流儀（テンプレート、CONTRIBUTING）に従い、このスキルは使わない。
`unset` は、最初に `docs/adr/` へ書くときに hook が remote から判定して書き込む。自分の組織など、自分のアカウント
以外の GitHub のリポジトリを自分のものとして扱うなら、`git config --global --add claude-hooks.owner <組織>` で足す。

## ADR にするもの

範囲、期間、危険、費用のどれかが大きい決定を ADR にする。範囲の小さい決定、規約で決まっていること、一時的な
回避策や実験は、issue の「経緯」か、コミットの本文に書く（事例集）。

## ファイルと題名

- 置き場所は `docs/adr/NNNN-verb-phrase.md`。4 桁の連番と、決めたことを動詞で始めた英語の kebab-case
  （例: `0005-adopt-madr-for-adrs.md`）。番号は既存の最大 + 1 で、一度使った番号は使い回さない
- 題名は `# ADR-NNNN: 決めたことを 1 文で`
- 一覧のファイルは作らない。採用中の ADR は `grep -l '^status: accepted$' docs/adr/[0-9]*.md` で引く

!`git ls-files docs/adr`

## 前付け

MADR の前付けの代わりに、次を使う。キーは 1 行に 1 つ、一覧は `[a, b]` の形で 1 行に書く（grep で引けるように）。

```yaml
---
status: proposed      # proposed | accepted | rejected | withdrawn | deferred | superseded | deprecated
date: 2026-10-11      # 最後に status が変わった日
requires: []          # 前提にしている ADR（例: [ADR-0002]）
supersedes: []        # この ADR が置き換えた ADR
superseded-by:        # この ADR を置き換えた ADR
issues: []            # きっかけになった issue と、この ADR で解決した issue
---
```

- `withdrawn` は決める前に取り下げたもの、`rejected` は決めて採らなかったもの、`deferred` は決めるのを後回しに
  したもの（PEP、KEP）
- `requires` に挙げた ADR を置き換えるときは、`grep -l 'requires:.*ADR-NNNN' docs/adr` で見直す ADR を引く（PEP）
- `supersedes` と `superseded-by` は両方の ADR に書く（古い ADR だけを読んでも置き換えに気づけるように）

## 本文

見出しは次の日本語で書く（hook は MADR の英語の原文も受け付ける）。必須の節が無ければ git の pre-commit が
コミットを止め、推奨の節が無ければ警告する（`claude-hooks adr-sections`）。任意の節は、要るときだけ書く。

| MADR                          | 見出し           |      |
| ----------------------------- | ---------------- | ---- |
| Context and Problem Statement | 背景             | 必須 |
| Decision Drivers              | 決定の要因       | 任意 |
| Considered Options            | 検討した案       | 必須 |
| Decision Outcome              | 決定と理由       | 必須 |
| Consequences                  | 帰結             | 推奨 |
| Confirmation                  | 確認             | 推奨 |
| Pros and Cons of the Options  | 各案の長所と短所 | 任意 |
| More Information              | 補足             | 任意 |

- 決まった言い回しも日本語にする: 「採用: …。理由は…」「良い点: …」「悪い点: …」「どちらでもない: …」
- 測った値や、変わりうる値（費用、規模）には、出典のファイルとコミットと日付を添える（xlc、事例集）
- 「帰結」に、あとで見直す条件を書く（MADR、xlc）
- 実装が決定に追いついていないときは、その差を「帰結」に書き、issue を立てて `issues` に入れる（xlc）

MADR 4.0.0 の `template/adr-template.md`（ライセンスは MIT OR CC0-1.0）。見出しと前付けは上の調整に読み替える:

```markdown
---
# These are optional metadata elements. Feel free to remove any of them.
status: "{proposed | rejected | accepted | deprecated | … | superseded by ADR-0123"
date: {YYYY-MM-DD when the decision was last updated}
decision-makers: {list everyone involved in the decision}
consulted: {list everyone whose opinions are sought (typically subject-matter experts); and with whom there is a two-way communication}
informed: {list everyone who is kept up-to-date on progress; and with whom there is a one-way communication}
---

# {short title, representative of solved problem and found solution}

## Context and Problem Statement

{Describe the context and problem statement, e.g., in free form using two to three sentences or in the form of an illustrative story. You may want to articulate the problem in form of a question and add links to collaboration boards or issue management systems.}

<!-- This is an optional element. Feel free to remove. -->
## Decision Drivers

* {decision driver 1, e.g., a force, facing concern, …}
* {decision driver 2, e.g., a force, facing concern, …}
* … <!-- numbers of drivers can vary -->

## Considered Options

* {title of option 1}
* {title of option 2}
* {title of option 3}
* … <!-- numbers of options can vary -->

## Decision Outcome

Chosen option: "{title of option 1}", because {justification. e.g., only option, which meets k.o. criterion decision driver | which resolves force {force} | … | comes out best (see below)}.

<!-- This is an optional element. Feel free to remove. -->
### Consequences

* Good, because {positive consequence, e.g., improvement of one or more desired qualities, …}
* Bad, because {negative consequence, e.g., compromising one or more desired qualities, …}
* … <!-- numbers of consequences can vary -->

<!-- This is an optional element. Feel free to remove. -->
### Confirmation

{Describe how the implementation of/compliance with the ADR can/will be confirmed. Are the design that was decided for and its implementation in line with the decision made? E.g., a design/code review or a test with a library such as ArchUnit can help validate this. Not that although we classify this element as optional, it is included in many ADRs.}

<!-- This is an optional element. Feel free to remove. -->
## Pros and Cons of the Options

### {title of option 1}

<!-- This is an optional element. Feel free to remove. -->
{example | description | pointer to more information | …}

* Good, because {argument a}
* Good, because {argument b}
<!-- use "neutral" if the given argument weights neither for good nor bad -->
* Neutral, because {argument c}
* Bad, because {argument d}
* … <!-- numbers of pros and cons can vary -->

### {title of other option}

{example | description | pointer to more information | …}

* Good, because {argument a}
* Good, because {argument b}
* Neutral, because {argument c}
* Bad, because {argument d}
* …

<!-- This is an optional element. Feel free to remove. -->
## More Information

{You might want to provide additional evidence/confidence for the decision outcome here and/or document the team agreement on the decision and/or define when/how this decision the decision should be realized and if/when it should be re-visited. Links to other decisions and resources might appear here as well.}
```

## 運用

決定の中身は変えず、変えるときは新しい ADR で置き換える（MADR、AWS の手引き、PEP 1、xlc）。

- **proposed の間**: 実装の前に proposed で書き、自由に直す。実装をマージするブランチで、本文（「確認」の実際の
  テストと結果、「帰結」）を実装に合わせて直し、同じ変更で `status: accepted` にして `date` をその日にする（xlc、PEP）
- **確定した後**（accepted、rejected、withdrawn、superseded）に新しい ADR なしで直すのは、次だけ:
  - 誤字、文法、マークアップ、壊れたリンク（PEP 1、xlc）
  - 前付けの `status`、`superseded-by`、`date`（AWS の手引き、xlc）
  - 「補足」の節への、日付つきの追記（`- 2026-11-02: …`）。決定の後に分かったことのうち、決定を変えないもの
    （事例集）
- **決定を変えるとき**: 新しい ADR を書き、新しい方の `supersedes` と古い方の `superseded-by` で結び、古い方を
  `status: superseded` にする。新しい ADR は、古い決定のうちまだ有効な部分を書き直して含める（採用中の ADR だけで
  今の決定が分かるように。xlc）
- main にマージする前は、ブランチの上の ADR を status によらず直してよい（xlc）
