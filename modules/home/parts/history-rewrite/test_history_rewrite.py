"""history-rewrite が一時的なリポジトリの履歴の語を直し、番号を付け直し、照合を通すこと（ADR-0045）。

PATH の history-rewrite、commit-refs、textlint を使う（package.nix の installCheckPhase が渡す）。nix の sandbox には
git の全体設定が無いので、init.defaultBranch と user をこのテストが与える。辞書の語（崩れた語）をわざと含むので、
.textlintignore で ja-terms の検査から外す。
"""

import json
import os
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

import history_rewrite


class Repo:
    def __init__(self, path: Path, env: dict):
        self.path = path
        self.env = env

    def git(self, *args: str, input: str | None = None) -> str:
        result = subprocess.run(
            ["git", "-C", str(self.path), *args],
            capture_output=True,
            text=True,
            env=self.env,
            input=input,
            check=True,
        )
        return result.stdout

    def write(self, name: str, content: str | bytes) -> None:
        path = self.path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content, encoding="utf-8")

    def commit(self, message: str) -> str:
        self.git("add", "-A")
        self.git("commit", "-q", "--no-verify", "-F", "-", input=message)
        return self.git("rev-parse", "HEAD").strip()

    def show(self, rev: str, path: str) -> bytes:
        return subprocess.run(
            ["git", "-C", str(self.path), "show", f"{rev}:{path}"],
            capture_output=True,
            env=self.env,
            check=True,
        ).stdout


class HistoryRewrite(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(cls.tmp.name)
        home = root / "home"
        home.mkdir()
        (home / ".gitconfig").write_text(
            "[init]\n\tdefaultBranch = main\n[user]\n\tname = t\n\temail = t@t\n"
            "[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n"
        )
        env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        env["HOME"] = str(home)
        env["GIT_CONFIG_NOSYSTEM"] = "1"
        cls.env = env
        repo = Repo(root / "source", env)
        repo.path.mkdir()
        repo.git("init", "-q")
        cls.repo = repo

        repo.write(
            "README.md",
            textwrap.dedent("""\
                # 手順

                確かめの結果を書く。結果を確かめる。

                | 項目 | 内容 |
                | ---- | ---- |
                | 幅 | 掃引の幅 |
                """),
        )
        repo.write("tool.py", "# 確かめの順に掃引する\nprint('ok')\n")
        repo.write("data.bin", b"\0\x01" + "確かめの".encode())
        repo.write("dict/terms.yml", "pattern: 確かめの\n")
        os.symlink("README.md", repo.path / "link.md")
        cls.first = repo.commit("docs: 確かめの手順を書く\n")

        repo.write(
            "notes.md",
            f"手順は {cls.first[:8]} と {cls.first} にある。旧称を使う。\n",
        )
        cls.second = repo.commit(
            f"docs: 記録を足す\n\n{cls.first[:7]} の確かめの結果。\n"
        )

        # 語を手で直しただけのコミット（変換の後は、親と同じ中身の空のコミットになる）
        repo.write("tool.py", "# 確認の順に掃引する\nprint('ok')\n")
        cls.manual = repo.commit("docs: 語を直す\n")

        repo.git("switch", "-q", "-c", "side", cls.second)
        repo.write("side.md", f"{cls.manual[:10]} の後の確かめの記録\n")
        cls.side = repo.commit("docs: 別のブランチ\n")
        repo.git("tag", "-a", "v1", "-m", f"確かめの版 {cls.side[:7]}", cls.side)
        repo.git("tag", "light", cls.first)
        # 署名つきのタグ（filter-repo は署名を外す）と、tagger の無いタグ（filter-repo は仮の tagger を付ける）
        repo.git(
            "tag",
            "-a",
            "signed",
            "-m",
            "確かめの版\n-----BEGIN PGP SIGNATURE-----\n\nAAAA\n-----END PGP SIGNATURE-----",
            cls.side,
        )
        raw = f"object {cls.side}\ntype commit\ntag bare\n\n確かめの版\n"
        bare = repo.git(
            "hash-object", "-t", "tag", "-w", "--literally", "--stdin", input=raw
        ).strip()
        repo.git("update-ref", "refs/tags/bare", bare)

        repo.git("switch", "-q", "-c", "extra", cls.manual)
        repo.write("latin1.txt", b"caf\xe9 \xe7a\n")
        repo.write(
            "fence.md",
            "```text\n| a | b |\n| - | - |\n| 掃引 | x |\n```\n",
        )
        repo.write("crlf.md", "| a | b |\r\n| - | - |\r\n| 掃引 | x |\r\n")
        repo.git("add", "-A")
        repo.git(
            "-c",
            "i18n.commitEncoding=ISO-8859-1",
            "commit",
            "-q",
            "--no-verify",
            "-m",
            "docs: ascii only",
        )
        cls.extra_tip = repo.git("rev-parse", "HEAD").strip()
        repo.git("switch", "-q", "main")

        cls.refs_before = repo.git("for-each-ref", "--format=%(refname) %(objectname)")
        extra = root / "extra.yml"
        extra.write_text("version: 1\nrules:\n  - expected: 新称\n    pattern: 旧称\n")
        cls.extra = extra
        cls.workdir = root / "work"
        cls.result = subprocess.run(
            [
                "history-rewrite",
                "run",
                "--source",
                str(repo.path),
                "--workdir",
                str(cls.workdir),
                "--extra-rules",
                str(extra),
                "--exclude",
                "**/terms.yml",
            ],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )
        cls.out = dict(
            line.split(": ", 1)
            for line in cls.result.stdout.splitlines()
            if ": " in line
        )
        cls.rewritten = Repo(cls.workdir / "rewritten", env)
        if cls.result.returncode == 0:
            cls.map = history_rewrite.read_map(cls.workdir / "commit-map")

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def setUp(self):
        self.assertEqual(self.result.returncode, 0, self.result.stderr)

    def test_words_are_fixed_in_documents_code_and_tables(self):
        readme = self.rewritten.show("main", "README.md").decode()
        self.assertIn("確認の結果を書く。結果を確かめる。", readme)
        self.assertIn("| 幅   | スイープの幅 |", readme)
        self.assertIn("| ---- | ------------ |", readme)
        tool = self.rewritten.show(self.map[self.first], "tool.py").decode()
        self.assertEqual(tool, "# 確認の順にスイープする\nprint('ok')\n")

    def test_extra_rules_apply_with_the_dictionary(self):
        notes = self.rewritten.show("main", "notes.md").decode()
        self.assertIn("新称を使う", notes)

    def test_hashes_in_documents_and_messages_point_at_the_final_commits(self):
        first = self.map[self.first]
        notes = self.rewritten.show("main", "notes.md").decode()
        self.assertIn(f"手順は {first[:8]} と {first} にある", notes)
        body = self.rewritten.git("log", "-1", "--format=%B", self.map[self.second])
        self.assertIn(f"{first[:7]} の確認の結果。", body)
        side = self.rewritten.show("side", "side.md").decode()
        self.assertIn(self.map[self.manual][:10], side)
        self.assertEqual(
            self.rewritten.git("log", "-1", "--format=%s", first).strip(),
            "docs: 確認の手順を書く",
        )

    def test_every_original_commit_maps_to_a_commit_of_the_rewritten_history(self):
        commits = self.repo.git("rev-list", "--all").split()
        self.assertEqual(sorted(self.map), sorted(commits))
        # 元の ref は commit-refs が refs/commit-refs/original/ に残すので、ブランチとタグだけを見る
        rewritten = set(self.rewritten.git("rev-list", "--branches", "--tags").split())
        self.assertEqual(set(self.map.values()), rewritten)
        self.assertEqual(self.out["COMMITS"], str(len(commits)))

    def test_branches_and_tags_move_to_the_mapped_commits(self):
        self.assertEqual(
            self.rewritten.git("rev-parse", "main").strip(), self.map[self.manual]
        )
        self.assertEqual(
            self.rewritten.git("rev-parse", "side").strip(), self.map[self.side]
        )
        self.assertEqual(
            self.rewritten.git("rev-parse", "light").strip(), self.map[self.first]
        )
        self.assertEqual(
            self.rewritten.git("rev-parse", "v1^{commit}").strip(), self.map[self.side]
        )
        message = self.rewritten.git("tag", "-l", "--format=%(contents)", "v1").strip()
        self.assertEqual(message, f"確認の版 {self.map[self.side][:7]}")

    def test_signed_and_taggerless_tags_follow_filter_repo(self):
        signed = self.rewritten.git("cat-file", "tag", "signed")
        self.assertTrue(signed.endswith("\n\n確認の版\n"), signed)
        bare = self.rewritten.git("cat-file", "tag", "bare")
        self.assertIn(f"object {self.map[self.side]}\n", bare)
        self.assertTrue(bare.endswith("\n\n確認の版\n"), bare)

    def test_commit_encoding_is_kept(self):
        raw = self.rewritten.git("cat-file", "commit", self.map[self.extra_tip])
        self.assertIn("\nencoding ISO-8859-1\n", raw)

    def test_text_that_is_not_utf8_is_counted_once_and_kept(self):
        self.assertEqual(self.out["NOT_UTF8"], "1")
        self.assertEqual(
            self.rewritten.show(self.map[self.extra_tip], "latin1.txt"),
            self.repo.show(self.extra_tip, "latin1.txt"),
        )

    def test_tables_in_code_fences_are_not_realigned(self):
        fence = self.rewritten.show(self.map[self.extra_tip], "fence.md").decode()
        self.assertEqual(
            fence, "```text\n| a | b |\n| - | - |\n| スイープ | x |\n```\n"
        )

    def test_realigned_tables_keep_crlf_line_endings(self):
        crlf = self.rewritten.show(self.map[self.extra_tip], "crlf.md").decode()
        self.assertEqual(crlf.count("\r\n"), 3, repr(crlf))
        self.assertIn("スイープ", crlf)

    def test_binary_symlink_and_excluded_files_are_kept(self):
        for path in ("data.bin", "link.md", "dict/terms.yml"):
            with self.subTest(path=path):
                self.assertEqual(
                    self.rewritten.show(self.map[self.first], path),
                    self.repo.show(self.first, path),
                )

    def test_the_source_repository_is_not_touched(self):
        self.assertEqual(
            self.repo.git("for-each-ref", "--format=%(refname) %(objectname)"),
            self.refs_before,
        )

    def lint(self, repo: Repo, commits: list[str]) -> subprocess.CompletedProcess:
        """コミットの .md と .py と本文に、同じ辞書の textlint（prh）を当てる"""
        config = Path(self.tmp.name) / "textlintrc.json"
        config.write_text(
            json.dumps(
                {
                    "plugins": {"@textlint/text": {"extensions": [".py"]}},
                    "rules": {
                        "prh": {
                            "rulePaths": [
                                os.environ["HISTORY_REWRITE_DICTIONARY"],
                                str(self.extra),
                            ]
                        }
                    },
                }
            )
        )
        files = Path(tempfile.mkdtemp(dir=self.tmp.name))
        n = 0
        for commit in commits:
            for name in repo.git("ls-tree", "-r", "--name-only", commit).splitlines():
                if name.endswith((".md", ".py")) and name != "link.md":
                    (files / f"{n}-{Path(name).name}").write_bytes(
                        repo.show(commit, name)
                    )
                    n += 1
            body = repo.git("log", "-1", "--format=%B", commit)
            (files / f"{n}-message.md").write_text(body, encoding="utf-8")
            n += 1
        return subprocess.run(
            [
                "textlint",
                "--config",
                str(config),
                "--format",
                "unix",
                "--no-color",
                ".",
            ],
            cwd=files,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_textlint_with_the_same_dictionary_finds_nothing_after_the_rewrite(self):
        before = self.lint(self.repo, list(self.map))
        self.assertNotEqual(
            before.returncode, 0, "書き換える前の履歴に、辞書の語が無い"
        )
        after = self.lint(self.rewritten, list(self.map.values()))
        self.assertEqual(after.returncode, 0, after.stdout + after.stderr)

    def test_verification_rejects_a_history_that_differs_from_the_transform(self):
        # 第 1 段の結果を、ブロブを 1 つ余分に直す変換と突き合わせると、照合が止める
        rules = history_rewrite.prh_rules.load(
            [Path(os.environ["HISTORY_REWRITE_DICTIONARY"]), self.extra]
        )
        rules += history_rewrite.prh_rules.load([self.write_rules("内容", "項目")])
        transformer = history_rewrite.Transformer(
            rules, ["**/terms.yml"], history_rewrite.load_table_format()
        )
        stage1 = history_rewrite.read_map(
            self.rewritten.path / ".git" / "filter-repo" / "commit-map"
        )
        verifier = history_rewrite.Verifier(
            self.rewritten.path, self.workdir / "original.git", stage1, transformer
        )
        with self.assertRaises(history_rewrite.Stop) as caught:
            verifier.run()
        self.assertIn("README.md", str(caught.exception))

    def test_verification_rejects_a_map_that_misses_an_original_commit(self):
        transformer = history_rewrite.Transformer(
            history_rewrite.prh_rules.load(
                [Path(os.environ["HISTORY_REWRITE_DICTIONARY"]), self.extra]
            ),
            ["**/terms.yml"],
            history_rewrite.load_table_format(),
        )
        stage1 = history_rewrite.read_map(
            self.rewritten.path / ".git" / "filter-repo" / "commit-map"
        )
        del stage1[self.second]
        verifier = history_rewrite.Verifier(
            self.rewritten.path, self.workdir / "original.git", stage1, transformer
        )
        with self.assertRaises(history_rewrite.Stop) as caught:
            verifier.run()
        self.assertIn(self.second, str(caught.exception))

    def test_a_workdir_inside_the_source_or_a_file_stops(self):
        afile = Path(self.tmp.name) / "afile"
        afile.write_text("")
        for workdir in (self.repo.path / "work", afile):
            with self.subTest(workdir=workdir):
                result = subprocess.run(
                    [
                        "history-rewrite",
                        "run",
                        "--source",
                        str(self.repo.path),
                        "--workdir",
                        str(workdir),
                    ],
                    capture_output=True,
                    text=True,
                    env=self.env,
                    check=False,
                )
                self.assertEqual(result.returncode, 1)
                self.assertIn("ERROR:", result.stderr)
        self.assertFalse((self.repo.path / "work").exists())

    def write_rules(self, expected: str, pattern: str) -> Path:
        path = Path(self.tmp.name) / f"rules-{expected}.yml"
        path.write_text(
            f"version: 1\nrules:\n  - expected: {expected}\n    pattern: {pattern}\n"
        )
        return path

    def test_a_non_empty_workdir_stops_without_rewriting(self):
        result = subprocess.run(
            [
                "history-rewrite",
                "run",
                "--source",
                str(self.repo.path),
                "--workdir",
                str(self.workdir),
            ],
            capture_output=True,
            text=True,
            env=self.env,
            check=False,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("空でない", result.stderr)

    def test_transform_fixes_working_tree_files_in_place(self):
        work = Repo(Path(self.tmp.name) / "transform", self.env)
        work.path.mkdir()
        work.git("init", "-q")
        work.write("doc.md", "確かめの記録\n")
        work.write("keep.md", "結果を確かめる\n")
        result = subprocess.run(
            ["history-rewrite", "transform", "doc.md", "keep.md"],
            cwd=work.path,
            capture_output=True,
            text=True,
            env=self.env,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((work.path / "doc.md").read_text(), "確認の記録\n")
        self.assertEqual((work.path / "keep.md").read_text(), "結果を確かめる\n")
        self.assertEqual(result.stdout, "REPLACED: doc.md\nFILES: 1\n")

    def test_transform_stops_on_a_missing_file_and_skips_symlinks(self):
        work = Repo(Path(self.tmp.name) / "transform-links", self.env)
        work.path.mkdir()
        work.git("init", "-q")
        work.write("doc.md", "確かめの記録\n")
        os.symlink("doc.md", work.path / "link.md")

        def transform(*files):
            return subprocess.run(
                ["history-rewrite", "transform", *files],
                cwd=work.path,
                capture_output=True,
                text=True,
                env=self.env,
                check=False,
            )

        # symlink を経たディレクトリの絶対パスでも、リポジトリの中のファイルとして直す
        alias = Path(self.tmp.name) / "alias"
        os.symlink(work.path, alias)
        via = transform(str(alias / "doc.md"))
        self.assertEqual(via.returncode, 0, via.stderr)
        self.assertEqual(via.stdout, "REPLACED: doc.md\nFILES: 1\n")
        (work.path / "doc.md").write_text("確かめの記録\n")

        missing = transform("gone.md")
        self.assertEqual(missing.returncode, 1)
        self.assertIn("ERROR:", missing.stderr)
        linked = transform("link.md")
        self.assertEqual(linked.returncode, 0, linked.stderr)
        self.assertEqual(linked.stdout, "FILES: 0\n")
        self.assertEqual((work.path / "doc.md").read_text(), "確かめの記録\n")


if __name__ == "__main__":
    unittest.main()
