#!/usr/bin/env python3
"""kept-sentences.py — 規則の文書を縮めたとき、今の版の文のうち元の版に無いものを出す

Usage: kept-sentences.py --old <元の版>... --new <今の版>...
  --old  縮める前の版（複数なら 1 つにつないで比べる）。git の版は `<(git show <base>:<path>)` で渡す
  --new  縮めた後の版（文を別のファイルに移したときは、移した先も並べる）

規則の文書を縮めるときは、文を消すか、ほかの置き場を指す形に替えるだけにし、残す文の字句を変えない
（言い換えると、どの項や量に掛かるか（主語、否定、条件）がずれ、レビューの周ごとに食い違いが出る）。
今の版の文ごとに、元の版の同じ塊（段落、箇条書きの項、見出し、表の行、コードブロックの行）の中の
部分文字列かを見る。比べる前に空白（行の折り返しと字下げを含む）を除き、箇条書きの印、見出しの #、
引用の > を除く。文は「。」「？」「！」で切る（括弧の中の「。」でも切る。切った部分はどれも元の
部分文字列なので、文の数え方で結果は変わらない）。

出力（元に無い文ごと）:
  CHANGED: <今の版のファイル>:<文の始まる行>: <文>
指す先を替えた文も出るので、出た文ごとに、指す形に替えたものか、元の文に戻すかを決める。

終了コード: 0 どの文も元の版にある / 1 元に無い文がある / 2 使い方の誤り、ファイルが読めない
"""

import argparse
import re
import sys

LIST_MARK = re.compile(r"^\s*(?:[-*+]|\d+\.)\s+")
HEADING = re.compile(r"^\s*#{1,6}\s+")
QUOTE_MARK = re.compile(r"^\s*>\s?")
FENCE = re.compile(r"^\s*(```|~~~)")
SENTENCE_END = "。？！"


def blocks(text):
    """塊ごとに、[(行番号, 印を除いた行)] を返す"""
    out = []
    cur = []
    fence = None

    def flush():
        nonlocal cur
        if cur:
            out.append(cur)
        cur = []

    for no, raw in enumerate(text.split("\n"), start=1):
        if fence is not None:
            if FENCE.match(raw) and raw.strip().startswith(fence):
                fence = None
            else:
                out.append([(no, raw)])
            continue
        m = FENCE.match(raw)
        if m:
            flush()
            fence = m.group(1)
            continue
        if not raw.strip():
            flush()
            continue
        if HEADING.match(raw) or raw.lstrip().startswith("|"):
            flush()
            out.append([(no, HEADING.sub("", raw))])
            continue
        line = QUOTE_MARK.sub("", raw)
        if LIST_MARK.match(line):
            flush()
            line = LIST_MARK.sub("", line)
        cur.append((no, line))
    flush()
    return out


def squeeze(s):
    return re.sub(r"\s+", "", s)


def sentences(block):
    """塊を文に切り、(文の始まる行番号, 文) を返す"""
    out = []
    start = None
    buf = ""
    for no, line in block:
        part = line.strip()
        for ch in part:
            if start is None:
                start = no
            buf += ch
            if ch in SENTENCE_END:
                out.append((start, buf))
                start, buf = None, ""
        if buf and buf[-1].isascii():
            buf += " "
    if buf.strip():
        out.append((start, buf))
    return [(no, s.strip()) for no, s in out]


def read(path):
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except OSError as e:
        print(f"ERROR: 読めない: {path}: {e}", file=sys.stderr)
        sys.exit(2)


def main():
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument("--old", nargs="+", required=True)
    parser.add_argument("--new", nargs="+", required=True)
    try:
        args = parser.parse_args()
    except SystemExit:
        sys.exit(2)

    # 塊の境目を越えて一致させない（別の段落の文をつないだ文を見つけるため）
    corpus = "\0".join(
        squeeze("".join(line for _, line in b))
        for path in args.old
        for b in blocks(read(path))
    )
    changed = 0
    for path in args.new:
        for b in blocks(read(path)):
            for no, sentence in sentences(b):
                key = squeeze(sentence)
                if key and key not in corpus:
                    print(f"CHANGED: {path}:{no}: {sentence}")
                    changed += 1
    sys.exit(1 if changed else 0)


if __name__ == "__main__":
    main()
