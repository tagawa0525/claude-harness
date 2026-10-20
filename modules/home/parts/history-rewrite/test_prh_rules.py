"""prh_rules が prh 6.0.0 と同じ意味で辞書を読み、文章に当てること（ADR-0045）。

辞書の語（崩れた語）をわざと含むので、.textlintignore で ja-terms の検査から外す。
"""

import os
import tempfile
import textwrap
import unittest
from pathlib import Path

import prh_rules


def load_text(yaml_text: str) -> list[prh_rules.Rule]:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "prh.yml"
        path.write_text(textwrap.dedent(yaml_text), encoding="utf-8")
        return prh_rules.load([path])


def rule(expected: str, pattern: str) -> list[prh_rules.Rule]:
    return load_text(f"""\
        version: 1
        rules:
          - expected: '{expected}'
            pattern: '{pattern}'
        """)


class NixfilesDictionary(unittest.TestCase):
    def test_loads_the_ja_terms_dictionary_and_its_specs_pass(self):
        rules = prh_rules.load([Path(os.environ["HISTORY_REWRITE_DICTIONARY"])])
        self.assertGreater(len(rules), 0)
        self.assertEqual(
            prh_rules.apply(rules, "確かめの結果を確かめる"), "確認の結果を確かめる"
        )


class Patterns(unittest.TestCase):
    def test_variable_width_lookbehind_with_line_start_reads_like_javascript(self):
        rules = rule("漏れ", "/(?<=[にがの、]|^)抜け(?=[をの])/")
        self.assertEqual(
            prh_rules.apply(rules, "判定に抜けを\n抜けの"), "判定に漏れを\n漏れの"
        )
        self.assertEqual(prh_rules.apply(rules, "すり抜けを"), "すり抜けを")

    def test_line_start_follows_carriage_return_as_in_javascript(self):
        rules = rule("X", "/^a/")
        self.assertEqual(prh_rules.apply(rules, "b\ra"), "b\rX")

    def test_dot_does_not_match_line_terminators(self):
        rules = rule("X", "/a.b/")
        self.assertEqual(prh_rules.apply(rules, "a\rb a-b"), "a\rb X")

    def test_lookahead_may_see_a_newline_inside_the_paragraph(self):
        rules = rule("ラッパー", "/包み(?!\\n?込)/")
        self.assertEqual(
            prh_rules.apply(rules, "包み\n込む 包みを"), "包み\n込む ラッパーを"
        )

    def test_fixed_string_pattern_matches_literally(self):
        rules = rule("X", "a.b")
        self.assertEqual(prh_rules.apply(rules, "a.b axb"), "X axb")

    def test_patterns_list_matches_any_of_them(self):
        rules = load_text("""\
            version: 1
            rules:
              - expected: スイープ
                patterns: [掃引, /スウィープ/]
            """)
        self.assertEqual(
            prh_rules.apply(rules, "掃引とスウィープ"), "スイープとスイープ"
        )


class Replacement(unittest.TestCase):
    def test_dollar_groups_are_substituted_and_out_of_range_ones_stay(self):
        rules = rule("$1-$2-$0-$5", "/(a)(b)?c/")
        self.assertEqual(prh_rules.apply(rules, "ac"), "a--$0-$5")

    def test_a_match_equal_to_expected_is_left_alone(self):
        rules = rule("ウェブ", "/ウ[ェエ]ブ/")
        self.assertEqual(prh_rules.apply(rules, "ウェブとウエブ"), "ウェブとウェブ")

    def test_all_rules_match_the_original_text_not_each_others_output(self):
        rules = load_text("""\
            version: 1
            rules:
              - expected: b
                pattern: a
              - expected: c
                pattern: b
            """)
        self.assertEqual(prh_rules.apply(rules, "ab"), "bc")

    def test_overlaps_keep_the_longer_at_the_same_start_and_the_earlier_otherwise(self):
        rules = load_text("""\
            version: 1
            rules:
              - expected: S
                pattern: ab
              - expected: L
                pattern: abc
              - expected: Z
                pattern: cd
            """)
        self.assertEqual(prh_rules.apply(rules, "abcd"), "Ld")
        self.assertEqual(prh_rules.apply(rules, "xbcd ab"), "xbZ S")

    def test_prh_disable_is_read_as_plain_text(self):
        rules = rule("確認", "確かめ")
        self.assertEqual(
            prh_rules.apply(rules, "`prh:disable` の確かめの"),
            "`prh:disable` の確認の",
        )

    def test_lookbehind_does_not_see_the_previous_paragraph(self):
        rules = rule("計数", "/(?<=の)数え/")
        self.assertEqual(prh_rules.apply(rules, "の\n\n数えが"), "の\n\n数えが")
        self.assertEqual(prh_rules.apply(rules, "の\n\n件の数えが"), "の\n\n件の計数が")


class Refusals(unittest.TestCase):
    def assert_refused(self, yaml_text: str, fragment: str):
        with self.assertRaises(prh_rules.DictionaryError) as caught:
            load_text(yaml_text)
        self.assertIn(fragment, str(caught.exception))

    def test_class_shorthands_whose_ranges_differ_are_refused(self):
        for escape in ("\\d", "\\w", "\\s", "\\b"):
            with (
                self.subTest(escape=escape),
                self.assertRaises(prh_rules.DictionaryError),
            ):
                rule("X", f"/a{escape}/")

    def test_ignore_case_flag_is_refused(self):
        with self.assertRaises(prh_rules.DictionaryError):
            rule("X", "/a/i")

    def test_named_groups_and_backreferences_are_refused(self):
        for pattern in ("/(?<n>a)/", "/(a)\\1/", "/\\k<n>/", "/\\p{L}/"):
            with (
                self.subTest(pattern=pattern),
                self.assertRaises(prh_rules.DictionaryError),
            ):
                rule("X", pattern)

    def test_empty_class_and_unescaped_bracket_in_class_are_refused(self):
        for pattern in ("/[^]/", "/[]/", "/[[]/"):
            with (
                self.subTest(pattern=pattern),
                self.assertRaises(prh_rules.DictionaryError),
            ):
                rule("X", pattern)

    def test_unknown_keys_and_rules_without_pattern_are_refused(self):
        self.assert_refused(
            """\
            version: 1
            rules:
              - expected: X
                pattern: a
                options: {wordBoundary: true}
            """,
            "options",
        )
        self.assert_refused(
            """\
            version: 1
            rules:
              - expected: X
            """,
            "pattern",
        )
        self.assert_refused(
            """\
            version: 1
            imports: [other.yml]
            rules: []
            """,
            "imports",
        )

    def test_a_failing_spec_stops_loading(self):
        self.assert_refused(
            """\
            version: 1
            rules:
              - expected: 確認
                pattern: 確かめ
                specs:
                  - from: 確かめの
                    to: 確かめの
            """,
            "specs",
        )

    def test_empty_matches_are_refused(self):
        rules = rule("X", "/a*/")
        with self.assertRaises(prh_rules.TextError):
            prh_rules.apply(rules, "b")


if __name__ == "__main__":
    unittest.main()
