---
name: adr
description: ADR（docs/adr/）を書く・直すときに使う。MADR を基準にした形、前付け、確定後の運用。
allowed-tools:
  - Bash(~/.claude/skills/adr/scripts/new-adr.sh*)
---

# ADR

何をなぜ決めたかを、1 件 1 決定で残す。形は [MADR](https://adr.github.io/madr/) 4.0.0 に従い、MADR と違う点だけを
下に書く。

自分のプロジェクトの ADR はこの形で書く。他人のプロジェクトでは、そのプロジェクトの流儀に従う。既存の ADR が別の形の
自分のプロジェクトでは、新しい ADR からこの形で書く。

## ADR にするもの

作り直したときに他へ波及する決定（データの形式、モジュールの境界、外部への依存、検証の方法など）と、範囲、期間、危険、
費用のどれかが大きい決定。それ以外の決定は、issue かコミットの本文に書く。

試して捨てた方式は、何を試し、なぜ捨てたか（測った値や反例）を、関係する ADR の検討した案か補足、または issue に書く。
コミットの本文だけに置くと、後から引けない。

## 作る

```bash
~/.claude/skills/adr/scripts/new-adr.sh <slug> "<決めたことの 1 文>"
```

`docs/adr/NNNN-<slug>.md` を、次の番号、前付け、必須と推奨の見出しつきで作る。slug は決めたことを動詞で始めた英語の
kebab-case にする。

## MADR と違う点

- **見出しは日本語**（英語の原文でもよい）: 背景、決定の要因、検討した案、決定と理由、帰結、確認、各案の長所と短所、
  補足。必須は背景・検討した案・決定と理由、推奨は帰結・確認。ほかの節を足してもよい
- **前付け**のキーは `status`、`date` と次の 4 つ（MADR の `decision-makers` などに代えて、置き換えと参照を grep で
  引けるようにする）。一覧は `[a, b]` の形で 1 行に書く
  - `requires`: 前提にしている ADR（`[ADR-0002]`）
  - `supersedes` / `superseded-by`: 置き換えた ADR と、置き換えられた ADR。両方の ADR に書く
  - `issues`: きっかけの issue と、解決した issue（`["#12"]`）
- **status** は `proposed`、`accepted`、`rejected`、`withdrawn`（決める前に取り下げた）、`deferred`（後回しにした）、
  `superseded`、`deprecated`。`date` は最後に status が変わった日
- **帰結**: 実装が決定に追いついていなければ、その差を書き、issue を立てて `issues` に入れる
- **前の決定に戻す**とき: その理由と測った値を、新しい ADR の検討した案に書く
- **測った値や変わりうる値**には、出典のファイルとコミットと日付を添える
- 一覧のファイルは作らない。採用中の ADR は `grep -l '^status: accepted$' docs/adr/[0-9]*.md` で引く

## 確定後の運用

決定を変えるときは新しい ADR で置き換え、ADR は増えていく前提にする。

- **proposed と deferred の間**: 自由に直す。実装をマージするブランチで、帰結と確認を実装に合わせ、`status: accepted` にする
- **確定した後**（accepted、rejected、withdrawn、superseded、deprecated）に直してよいのは、誤字、文法、マークアップ、
  壊れたリンク、前付けの `status` / `superseded-by` / `date`、補足の節への日付つきの追記（`- YYYY-MM-DD: …`）だけ
- **置き換え**: 新しい ADR は、古い決定のうちまだ有効な部分を書き直して含める（採用中の ADR だけで今の決定が分かる
  ように）。古い方は `status: superseded` と `superseded-by` を書く。古い方を前提にしていた ADR を
  `grep -l 'requires:.*ADR-NNNN' docs/adr` で引き、見直す
- main にマージする前は、ブランチの上の ADR を status によらず直してよい
