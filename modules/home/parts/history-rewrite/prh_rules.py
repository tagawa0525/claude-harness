"""prh の辞書（ADR-0014 の prh.yml の形）を読み、prh 6.0.0 と同じ意味で文章に当てる（ADR-0045）。

一致の意味は prh の lib/engine.js、rule.js、paragraph.js、changeset/changeset.js、changeset/diff.js に揃える:

- 文章を `\\n{2,}` で段落に分け、段落ごとにすべての規則の一致を、元の文字列の上で集める
- 一致した文字列が expected と同じなら直さない。expected の `$1`〜`$99` は捕捉した群に置き換える
- 重なった一致は、同じ開始なら長い方、違う開始なら先の方を採る

prh の `prh:disable` の指示（paragraph.js）は当てない。textlint-rule-prh は Markdown の Str ノードごとに prh を呼ぶので、
指示の効く範囲はノードで決まり、文章の全文に当てるこのツールでは同じ範囲にならない（ADR-0045）。

pattern は JavaScript の正規表現（u フラグ）として読み、意味の同じ Python の `regex` の書き方に書き直す。意味が
違うか、書き直せない書き方は、推測せずに DictionaryError で止める。
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

import regex
import yaml


class DictionaryError(Exception):
    """辞書を読めない、扱わない書き方がある、specs が合わない"""


class TextError(Exception):
    """文章に、prh と同じ意味で当てられない箇所がある"""


# JavaScript の行の終わり（`^`、`$` の m フラグと `.` が見るもの）
_LINE_TERMINATORS = "\\n\\r\\u2028\\u2029"
# prh の rule.js の parseRegExpString（utils/regexp.js の regexpRegexp）。JavaScript の `.` は改行に当たらない
_REGEXP_STRING = re.compile(r"^/([^\n\r  ]*)/([gimy]*)$")
# prh の escapeSpecialChars が `\` を前に付ける文字（`\` 自身は含まない）
_PRH_SPECIAL = "¥*+.?{}()[]^$-|/"
# 構文の文字をそのまま表すエスケープ（u フラグで許されるもの）
_SYNTAX_ESCAPES = "^$\\.*+?()[]{}|/-"
_CONTROL_ESCAPES = {"n": "\\n", "r": "\\r", "t": "\\t", "f": "\\f", "v": "\\v"}
# prh の engine.js の段落の区切り。prh の `/([^]*?)\n{2,}/g` は、前の区切りの後から次の `\n{2,}` までを段落にする。
# 同じ書き方を Python の re で当てると、区切りの無い長い末尾で開始の位置ごとに読み直し、長さの 2 乗の時間が
# かかる（nixfiles の履歴の 3656 ブロブで 271 秒）ので、区切りだけを探す
_PARAGRAPH_BREAK = re.compile(r"\n{2,}")
_DOLLAR = re.compile(r"\$([0-9]{1,2})")
_RULE_KEYS = {"expected", "pattern", "patterns", "specs"}


@dataclass(frozen=True)
class Rule:
    expected: str
    pattern: regex.Pattern
    # 辞書のファイルと規則の位置（理由の表示に使う）
    origin: str


@dataclass(frozen=True)
class _Diff:
    index: int
    tail: int
    new_text: str


def translate(source: str) -> str:
    """JavaScript の正規表現（u フラグ、m フラグ）の source を、同じ意味の `regex` の pattern にする。

    `^`、`$`、`.` は JavaScript の行の終わり（\\n、\\r、U+2028、U+2029）で読み替える。次は DictionaryError で止める:
    `\\d`、`\\w`、`\\s`、`\\b` とその否定（当たる範囲が JavaScript と Python で違う）、後方参照、名前つきの群、
    Unicode の性質のエスケープ（書き直していない）。Python の re でなく regex を使うのは、prh の辞書が長さの違う
    選択肢の lookbehind（`(?<=[の、]|^)`）を使い、re は固定の長さの lookbehind しか読めないため。
    """
    out: list[str] = []
    i = 0
    n = len(source)
    in_class = False
    # 直前の字句が量指定子か（`*+` のような所有の量指定子は JavaScript では構文の誤り）
    after_quantifier = False

    def fail(reason: str) -> DictionaryError:
        return DictionaryError(f"/{source}/ の {i + 1} 文字目: {reason}")

    while i < n:
        c = source[i]
        if c == "\\":
            if i + 1 >= n:
                raise fail("末尾の \\")
            out.append(_translate_escape(source, i, fail))
            i += _escape_length(source, i)
            after_quantifier = False
            continue
        if in_class:
            if c == "]":
                in_class = False
            elif c == "[":
                raise fail(
                    "文字クラスの中の [ はエスケープする（Python では入れ子の集合と読む）"
                )
            out.append(c)
            i += 1
            continue
        if c == "[":
            start = i + 2 if source.startswith("[^", i) else i + 1
            if start < n and source[start] == "]":
                raise fail("空の文字クラス（[] と [^]）は Python と意味が違う")
            in_class = True
            out.append(source[i:start])
            i = start
            after_quantifier = False
            continue
        if c == "(":
            if source.startswith("(?", i):
                prefix = next(
                    (
                        p
                        for p in ("(?:", "(?=", "(?!", "(?<=", "(?<!")
                        if source.startswith(p, i)
                    ),
                    None,
                )
                if prefix is None:
                    raise fail("扱う群は (…)、(?:…)、(?=…)、(?!…)、(?<=…)、(?<!…) だけ")
                out.append(prefix)
                i += len(prefix)
            else:
                out.append(c)
                i += 1
            after_quantifier = False
            continue
        if c == "+" and after_quantifier:
            raise fail(
                "量指定子の後の + は JavaScript では構文の誤り（Python では所有の量指定子）"
            )
        if c == "^":
            out.append(f"(?:(?<=[{_LINE_TERMINATORS}])|\\A)")
        elif c == "$":
            out.append(f"(?=[{_LINE_TERMINATORS}]|\\Z)")
        elif c == ".":
            out.append(f"[^{_LINE_TERMINATORS}]")
        else:
            out.append(c)
        after_quantifier = c in "*+?}"
        i += 1
    if in_class:
        raise DictionaryError(f"/{source}/: 文字クラスが閉じていない")
    return "".join(out)


def _escape_length(source: str, i: int) -> int:
    c = source[i + 1]
    if c == "x":
        return 4
    if c == "u":
        return 6
    return 2


def _translate_escape(source: str, i: int, fail) -> str:
    c = source[i + 1]
    if c in _SYNTAX_ESCAPES:
        return "\\" + c
    if c in _CONTROL_ESCAPES:
        return _CONTROL_ESCAPES[c]
    if c == "0":
        if i + 2 < len(source) and source[i + 2].isdigit():
            raise fail("\\0 の後に数字が続く（8 進のエスケープは u フラグでは誤り）")
        return "\\x00"
    if c == "x":
        digits = source[i + 2 : i + 4]
        if len(digits) != 2 or not all(d in "0123456789abcdefABCDEF" for d in digits):
            raise fail("\\x の後は 16 進の 2 桁")
        return "\\x" + digits
    if c == "u":
        digits = source[i + 2 : i + 6]
        if len(digits) != 4 or not all(d in "0123456789abcdefABCDEF" for d in digits):
            raise fail("\\u の後は 16 進の 4 桁（\\u{…} は扱わない）")
        if 0xD800 <= int(digits, 16) <= 0xDFFF:
            raise fail(
                "サロゲートの \\u は JavaScript では 2 つで 1 文字になり、Python と意味が違う"
            )
        return "\\u" + digits
    if c in "dDwWsSbB":
        raise fail(
            f"\\{c} は JavaScript と Python で当たる範囲が違う（文字クラスで書く）"
        )
    raise fail(
        f"\\{c} は扱わない（後方参照、名前つきの群、Unicode の性質、制御文字のエスケープ）"
    )


def _pattern_source(pattern: object, origin: str) -> str:
    """prh の rule.js の _patternToRegExp に揃えて、pattern を JavaScript の source にする"""
    if isinstance(pattern, list):
        if not pattern:
            raise DictionaryError(f"{origin}: patterns が空")
        return "(?:" + "|".join(_pattern_source(p, origin) for p in pattern) + ")"
    if not isinstance(pattern, str):
        raise DictionaryError(f"{origin}: pattern は文字列か文字列の並び: {pattern!r}")
    if pattern == "":
        raise DictionaryError(f"{origin}: pattern が空")
    m = _REGEXP_STRING.match(pattern)
    if m:
        if "i" in m.group(2):
            raise DictionaryError(
                f"{origin}: i フラグは扱わない（大文字と小文字の畳み方が JavaScript と違う）"
            )
        # g と m は prh が必ず付け、y は外すので、意味を変えない
        return m.group(1)
    # 固定の文字列。prh は escapeSpecialChars で特別な文字の前に \ を付けて RegExp にする（\ 自身は付けない）
    if "¥" in pattern:
        raise DictionaryError(
            f"{origin}: 固定の文字列の ¥ は、prh が \\¥ にして u フラグの RegExp として読めない（/…/ で書く）"
        )
    return "".join("\\" + ch if ch in _PRH_SPECIAL else ch for ch in pattern)


def _compile(source: str, origin: str) -> regex.Pattern:
    try:
        return regex.compile(translate(source), regex.V0)
    except DictionaryError as e:
        raise DictionaryError(f"{origin}: {e}") from None
    except regex.error as e:
        raise DictionaryError(
            f"{origin}: /{source}/ を Python の regex で読めない: {e}"
        ) from None


def load(paths: list[Path]) -> list[Rule]:
    """辞書のファイルを順に読み、規則を並べる。どの規則も specs を確かめる（prh が辞書を読むときと同じ）"""
    rules: list[Rule] = []
    for path in paths:
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, yaml.YAMLError) as e:
            raise DictionaryError(f"{path}: 読めない: {e}") from None
        if not isinstance(data, dict):
            raise DictionaryError(f"{path}: 辞書の形（version と rules）でない")
        extra = set(data) - {"version", "rules"}
        if extra:
            raise DictionaryError(f"{path}: 扱わない項目: {', '.join(sorted(extra))}")
        if data.get("version", 1) != 1:
            raise DictionaryError(
                f"{path}: version は 1 だけを扱う: {data['version']!r}"
            )
        raw_rules = data.get("rules") or []
        if not isinstance(raw_rules, list):
            raise DictionaryError(f"{path}: rules が並びでない")
        for number, raw in enumerate(raw_rules, start=1):
            origin = f"{path} の {number} 番目の規則"
            rules.append(_load_rule(raw, origin))
    return rules


def _load_rule(raw: object, origin: str) -> Rule:
    if not isinstance(raw, dict):
        raise DictionaryError(
            f"{origin}: pattern の無い規則（文字列だけの規則）は扱わない"
        )
    extra = set(raw) - _RULE_KEYS
    if extra:
        raise DictionaryError(f"{origin}: 扱わない項目: {', '.join(sorted(extra))}")
    expected = raw.get("expected")
    if not isinstance(expected, str):
        raise DictionaryError(f"{origin}: expected が文字列でない")
    # prh は `rawRule.pattern || rawRule.patterns` を使う（空の文字列は先に止める）
    pattern = raw.get("pattern") or raw.get("patterns")
    if pattern is None:
        raise DictionaryError(f"{origin}: pattern の無い規則は扱わない")
    rule = Rule(expected, _compile(_pattern_source(pattern, origin), origin), origin)
    for spec in raw.get("specs") or []:
        if not (isinstance(spec, dict) and set(spec) == {"from", "to"}):
            raise DictionaryError(f"{origin}: specs の項目は from と to: {spec!r}")
        got = apply([rule], str(spec["from"]))
        if got != str(spec["to"]):
            raise DictionaryError(
                f"{origin}: specs が合わない: {spec['from']!r} は {spec['to']!r} のはずが {got!r}"
            )
    return rule


def apply(rules: list[Rule], text: str) -> str:
    """text に規則を prh と同じ意味で当てた結果"""
    diffs: list[_Diff] = []
    for offset, paragraph in _paragraphs(text):
        for rule in rules:
            for m in rule.pattern.finditer(paragraph):
                if m.end() == m.start():
                    # JavaScript の exec の繰り返しは、空の一致で先に進まない
                    raise TextError(f"{rule.origin}: 空の文字列に一致する")
                if m.group() == rule.expected:
                    continue
                start = offset + m.start()
                diffs.append(
                    _Diff(start, start + len(m.group()), _expand(rule.expected, m))
                )
    # prh は ChangeSet を作るときと当てるときの 2 度、重なりを外す
    diffs = _prepare(_prepare(diffs))
    out: list[str] = []
    last = 0
    for d in diffs:
        out.append(text[last : d.index])
        out.append(d.new_text)
        last = d.tail
    out.append(text[last:])
    return "".join(out)


def _paragraphs(text: str) -> list[tuple[int, str]]:
    result: list[tuple[int, str]] = []
    last = 0
    for m in _PARAGRAPH_BREAK.finditer(text):
        result.append((last, text[last : m.start()]))
        last = m.end()
    result.append((last, text[last:]))
    return result


def _expand(expected: str, m: regex.Match) -> str:
    """diff.js の newText: `$N` を N 番目の群に置き換える。0 と、群の数を超える N はそのまま残す"""

    def replace(d: re.Match) -> str:
        index = int(d.group(1))
        if index == 0 or index > len(m.groups()):
            return d.group()
        return m.group(index) or ""

    return _DOLLAR.sub(replace, expected)


def _prepare(diffs: list[_Diff]) -> list[_Diff]:
    """changeset.js の _prepare。並べ、同じ開始なら長い方を、違う開始なら先の方を残す"""
    ordered = sorted(diffs, key=lambda d: (d.index, d.tail))
    kept = [
        d
        for k, d in enumerate(ordered)
        if not (
            k + 1 < len(ordered)
            and d.index == ordered[k + 1].index
            and d.tail < ordered[k + 1].tail
        )
    ]
    # 前の要素は、この段で外す前の並び（JavaScript の filter は元の配列を見る）
    return [d for k, d in enumerate(kept) if not (k > 0 and d.index < kept[k - 1].tail)]
