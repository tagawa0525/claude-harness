---
name: adr
description: ADR（docs/adr/）を書く・直すときに使う。MADR 4.0.0 のテンプレートと、ADR に書かないもの。
allowed-tools:
  - Bash(git config*)
  - Bash(git ls-files*)
---

# ADR

何をなぜ決めたかを、1 件 1 決定で残す。形は [MADR](https://adr.github.io/madr/) 4.0.0 をそのまま使い、
調整は次の 4 つだけ（ADR-0005）:

- 置き場所は `docs/adr/NNNN-title-with-dashes.md`（MADR の既定は `docs/decisions/`）
- 見出しはテンプレートの英語のまま、本文は日本語で書く
- 下の「書かないもの」を守る
- 他人のプロジェクトでは使わない

## このリポジトリの扱い

!`git config --default unset --get claude-hooks.own-project`

`false` なら他人のプロジェクト。このスキルではなく、そのプロジェクトの流儀（テンプレート、CONTRIBUTING）に
従う。`unset` は、最初に `docs/adr/` へ書くときに hook が remote から判定して書き込む。自分の組織など、
自分のアカウント以外の GitHub のリポジトリを自分のものとして扱うなら、
`git config --global --add claude-hooks.owner <組織>` で足す。

## 手元の ADR を手本にしない

既存の ADR は形の手本ではなく、下のテンプレートに照らして評価する対象。欠陥を見つけたら真似ず、直す提案を
する（別のトピック。/topic-triage）。

## テンプレート

必須の節は Context and Problem Statement / Considered Options / Decision Outcome（git の pre-commit が
`claude-hooks adr-sections` で検査する）。任意の節は、要らなければテンプレートの指示どおり消す。

MADR 4.0.0 の `template/adr-template.md` を手を加えずに収めたもの（ライセンスは MIT OR CC0-1.0）:

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

## 書かないもの

| 書きたくなるもの                                           | 置き場所                                                                 |
| ---------------------------------------------------------- | ------------------------------------------------------------------------ |
| ホストやリポジトリの今の状態（何が残っている、何を消した） | 書かない。状態は変わり、ADR が古くなる                                   |
| 未解決の問題、やること                                     | issue                                                                    |
| コードや設定の写し                                         | コミットハッシュやファイルへの参照。写しは古くなる                       |
| 手順書（導入・再開の手順）                                 | 手順の置き場（README、docs、そのコードのリポジトリ）。ADR からは参照する |
| 実装の細部（競合状態、エッジケース）                       | 実装の PR でテストとともに                                               |

## 番号

!`git ls-files docs/adr`

最大の番号 + 1。1 ブランチに新しい ADR は 1 件（hook が 2 件以上を止める）。
