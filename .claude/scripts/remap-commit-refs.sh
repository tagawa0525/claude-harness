#!/usr/bin/env bash
# 履歴の書き換え（rebase、amend、git filter-repo）で変わったコミットの番号を、作業ツリーの
# 追跡しているテキストのファイルの中で付け直す。コミットはしない。
#
# 使い方: remap-commit-refs.sh [対応表]   （省略すると標準入力から読む）
#
# 対応表は 1 行に「旧 新」の完全な番号。次の 2 つの形を読む:
#   - git の post-rewrite hook の入力（3 列目以降があれば無視する）
#   - git filter-repo の .git/filter-repo/commit-map（見出しの行 `old new` は読み飛ばす。
#     新が 0 だけの行は書き換えで消えたコミット、旧と新が同じ行は変わらなかったコミット）
#
# 文書の中の、7 桁以上で単語の境界に挟まれた小文字の 16 進の語のうち、対応表の旧の番号の
# 先頭に一致するものを、同じ桁数の新の番号の先頭に置き換える。対応表にない語は変えない。
#
# 出力（KEY: value）:
#   REPLACED: <パス> <件数>   置き換えたファイルごと
#   FILES: <置き換えたファイルの数>
#   REFS: <置き換えた語の数>
# 次のときは何も書き換えず、ERROR: を出して exit 1:
#   対応表の行が読めない、同じ旧に別々の新が対応する、文書の語が複数の旧に当たる（曖昧）、
#   文書の語が書き換えで消えたコミットを指す

set -euo pipefail

if (( $# > 1 )); then
  echo "ERROR: 引数は対応表のファイル 1 つだけ（省略すると標準入力）" >&2
  exit 1
fi

TOP=$(git rev-parse --show-toplevel)

MAP=$(mktemp)
FILES=$(mktemp)
trap 'rm -f "$MAP" "$FILES"' EXIT

if (( $# == 1 )); then
  cp -- "$1" "$MAP"
else
  cat > "$MAP"
fi

# 7 桁の 16 進を含む、追跡しているテキストのファイル（-I でバイナリを除く）。
# git grep は一致がないと 1 を返すので、それだけを 0 件として扱う
rc=0
git -C "$TOP" grep -z -l -I -E '[0-9a-f]{7}' > "$FILES" || rc=$?
if (( rc > 1 )); then
  echo "ERROR: git grep が失敗した（終了コード $rc）" >&2
  exit 1
fi

cd "$TOP"
python3 - "$MAP" "$FILES" <<'PY'
import os
import re
import sys

map_path, files_path = sys.argv[1], sys.argv[2]
FULL = re.compile(r"[0-9a-f]{40}([0-9a-f]{24})?")
# \b は bytes の正規表現では ASCII の単語の境界（英数字と _）
TOKEN = re.compile(rb"\b[0-9a-f]{7,64}\b")
PREFIX = 7

errors = []
targets = {}  # 対応表のすべての旧 → 新
mapping = {}  # 旧 → 新（変わったものだけ）
pruned = set()  # 書き換えで消えた旧

with open(map_path, encoding="utf-8") as f:
    seen_entry = False
    for lineno, line in enumerate(f, 1):
        fields = line.split()
        if not fields:
            continue
        if not seen_entry and fields == ["old", "new"]:
            continue
        seen_entry = True
        if len(fields) < 2 or not FULL.fullmatch(fields[0]) or not FULL.fullmatch(fields[1]):
            errors.append(f"対応表の {lineno} 行目が「旧 新」の完全な番号でない: {line.rstrip()}")
            continue
        old, new = fields[0], fields[1]
        # 変わらない行と消えた行も含めて、旧ごとの新を先に揃える
        if targets.setdefault(old, new) != new:
            errors.append(f"旧 {old} に別々の新が対応する: {targets[old]} と {new}")

if errors:
    for e in errors:
        print(f"ERROR: {e}", file=sys.stderr)
    sys.exit(1)

for old, new in targets.items():
    if set(new) == {"0"}:
        pruned.add(old)
    elif old != new:
        mapping[old] = new

if errors:
    for e in errors:
        print(f"ERROR: {e}", file=sys.stderr)
    sys.exit(1)

# 先頭 7 桁で引く索引。消えたコミットも曖昧さの判定と参照の検出のために入れる
index = {}
for old in list(mapping) + sorted(pruned):
    index.setdefault(old[:PREFIX], []).append(old)

with open(files_path, "rb") as f:
    # git のパス名はバイト列で、UTF-8 とは限らない。OS のファイル名の符号化で戻せる形にする
    paths = [os.fsdecode(p) for p in f.read().split(b"\0") if p]
# 出力するパス名も同じバイト列に戻す
sys.stdout.reconfigure(errors="surrogateescape")
sys.stderr.reconfigure(errors="surrogateescape")

def remap(path, data):
    """data の中の旧の番号を置き換えた内容と件数を返す。曖昧な語と消えたコミットは errors に積む"""
    count = 0

    def replace(m):
        nonlocal count
        token = m.group().decode()
        olds = [o for o in index.get(token[:PREFIX], []) if o.startswith(token)]
        if not olds:
            return m.group()
        if len(olds) > 1:
            errors.append(f"{path}: {token} が複数の旧に当たる（曖昧）: {' '.join(olds)}")
            return m.group()
        old = olds[0]
        if old in pruned:
            errors.append(f"{path}: {token} は書き換えで消えたコミット {old} を指す")
            return m.group()
        count += 1
        return mapping[old][: len(token)].encode()

    return TOKEN.sub(replace, data), count


results = []  # (パス, 新しい内容, 件数)
for path in paths:
    # 追跡しているシンボリックリンクは、書くとリンク先を変えてしまうので飛ばす
    # （リンク先が追跡しているファイルなら、そちらとして置き換わる）
    if os.path.islink(path):
        continue
    with open(path, "rb") as f:
        new_data, count = remap(path, f.read())
    if count:
        results.append((path, new_data, count))

if errors:
    for e in errors:
        print(f"ERROR: {e}", file=sys.stderr)
    print("ERROR: 何も書き換えていない", file=sys.stderr)
    sys.exit(1)

for path, new_data, count in results:
    with open(path, "wb") as f:
        f.write(new_data)
    print(f"REPLACED: {path} {count}")
print(f"FILES: {len(results)}")
print(f"REFS: {sum(c for _, _, c in results)}")
PY
