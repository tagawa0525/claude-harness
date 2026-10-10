#!/usr/bin/env bash
# adr: ADR の雛形を作る
#
# Usage: new-adr.sh <slug> <title>
#
# slug:  決めたことを動詞で始めた英語の kebab-case（例: adopt-madr-for-adrs）
# title: 決めたことの 1 文（表題の「ADR-NNNN: 」の後ろ）
# Output: 作ったファイルの、リポジトリの直下からのパス
#
# 番号は、作業ツリーと全ブランチの履歴に現れた docs/adr/<数字>-*.md の最大 + 1 を 4 桁で振る
# （3 桁で振っていた既存の ADR も数える）。
# 削除した ADR の番号を使い回すと、古い参照が別の決定を指してしまうため、履歴も数える。

set -euo pipefail

if (($# != 2)) || [[ -z "$2" ]]; then
  echo "Usage: new-adr.sh <slug> <title>" >&2
  exit 1
fi
SLUG="$1"
TITLE="$2"

if [[ ! "$SLUG" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]]; then
  echo "slug は英小文字と数字の kebab-case にする: $SLUG" >&2
  exit 1
fi

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"
mkdir -p docs/adr

LAST=$(
  {
    git log --all --format= --name-only -- docs/adr
    find docs/adr -maxdepth 1 -name '[0-9]*-*.md'
  } | sed -nE 's|^docs/adr/([0-9]+)-[^/]*\.md$|\1|p' | sort -n | tail -1
)
NUM=$(printf '%04d' $((10#${LAST:-0} + 1)))
FILE="docs/adr/$NUM-$SLUG.md"

cat >"$FILE" <<ADR
---
status: proposed
date: $(date +%F)
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-$NUM: $TITLE

## 背景

## 検討した案

## 決定と理由

### 帰結

### 確認
ADR
echo "$FILE"
