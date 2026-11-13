#!/usr/bin/env bash
# 固定の入力（このディレクトリの *.markdown と *.txt。*.md にすると markdownlint が直して入力を変える）を textlint 15.7.1 と textlint-rule-prh 6.0.0 で読み、Str のノードごとの
# 開始の位置（UTF-16）と原文を <入力>.strs に書く。tests/strs.rs が Rust の切り出しをこれと比べる（ADR-0085）。
# report で始まる入力は、代わりに report.yml を当てた unix の形の報告を <入力>.unix に書く（パスは入力の名前。
# tests/check.rs が CLI の出力と比べる）。報告の入力は prh:disable を含み、prh がその段落（テキストは行）に whole.yml を
# 当てないので、Str の期待値を作れない。
#
# 使い方: update.sh <textlint の実行ファイル> [入力...]
#   textlint は textlint-rule-prh を含むもの（リポジトリの直下で
#   nix build --no-link --print-out-paths --impure --expr 'let p = (builtins.getFlake (toString ./.)).inputs.nixpkgs.legacyPackages.x86_64-linux; in p.textlint.withPackages [ p.textlint-rule-prh ]'
#   の bin/textlint）。入力を挙げなければ、すべてを作り直す
set -euo pipefail

textlint=$1
shift
dir=$(cd "$(dirname "$0")" && pwd -P)
cd "$dir"
if (($# == 0)); then
  set -- *.markdown *.txt
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf '{"plugins":{"@textlint/text":{"extensions":[".txt"]}},"rules":{"prh":{"rulePaths":["%s"]}}}\n' "$dir/whole.yml" >"$work/rc.json"
printf '{"plugins":{"@textlint/text":{"extensions":[".txt"]}},"rules":{"prh":{"rulePaths":["%s"]}}}\n' "$dir/report.yml" >"$work/report.json"
for input in "$@"; do
  if [[ $input == report* ]]; then
    rc=0
    "$textlint" --config "$work/report.json" --format unix --no-color "$input" >"$work/out.unix" || rc=$?
    if ((rc > 1)); then
      echo "textlint が失敗した: $input" >&2
      exit 2
    fi
    # unix の形は絶対パスで出すので、入力の名前に戻す
    while IFS= read -r line; do
      printf '%s\n' "${line#"$dir/"}"
    done <"$work/out.unix" >"$input.unix"
    continue
  fi
  rc=0
  "$textlint" --config "$work/rc.json" --format json "$input" >"$work/out.json" || rc=$?
  # 1 は報告がある（Str が 1 つ以上ある）、0 は Str が無い
  if ((rc > 1)); then
    echo "textlint が失敗した: $input" >&2
    exit 2
  fi
  python3 -I - "$work/out.json" >"$input.strs" <<'PY'
import json, sys
suffix = " => \0"
for result in json.load(open(sys.argv[1], encoding="utf-8")):
    for m in result["messages"]:
        source = m["message"]
        assert source.endswith(suffix), source
        source = source[: -len(suffix)]
        escaped = source.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
        print(f"{m['range'][0]}\t{escaped}")
PY
done
