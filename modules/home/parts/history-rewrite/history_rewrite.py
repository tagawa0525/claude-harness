"""history-rewrite: 履歴の中の崩れた語を辞書で直し、文書と本文のコミットの番号を付け直す（ADR-0045）。

Usage:
  history-rewrite run --source REPO --workdir DIR [語の指定]
      REPO のすべてのブランチとタグを DIR にコピーして書き換える。REPO には書き込まない。
      1. DIR/original.git に REPO のミラーを取る（書き換える前の控え）
      2. DIR/rewritten に、ミラーのブランチとタグを持つ作業ツリーつきのリポジトリを作る
      3. 第 1 段: git filter-repo の file-info-callback と message-callback で、テキストのブロブの全文と
         コミットとタグの本文の語を 1 回で直す（番号には触らない）
      4. 照合: 元のすべてのコミットと書き換えた後を 1 対 1 で突き合わせる（作者と日時、親、本文、すべてのファイル）
      5. 第 2 段: commit-refs rewrite-history（ADR-0013）で、文書と本文の番号を最後の番号に付け直す
      6. 元の番号から最後の番号への対応表を DIR/commit-map に書く
  history-rewrite transform [語の指定] FILE...
      作業ツリーのファイルの語を、run の第 1 段と同じ変換で直す（コミットはしない）
  history-rewrite sync-worktrees --workdir DIR [語の指定]
      run の結果の ref を元のリポジトリに移した後、DIR/worktree-state の行（worktree のパス、ref を移す前の
      HEAD、控えのパッチのパスをタブで区切る）ごとに、worktree の HEAD、index、作業ツリーを新しい履歴に合わせる。
      未コミットの変更は、ref を移す前の HEAD と比べて見つける（移した後の HEAD と比べると、書き換えたファイルが
      すべて変更に見える）
      - detached HEAD は DIR/commit-map の対応で移す
      - 変更の無い worktree は、新しい HEAD に reset --hard する（追跡していないファイルは残る）
      - 変更のある worktree は、commit-refs remap で番号を付け直し、書き換えたファイルのうち手を付けていないものを
        新しい中身にし、変えたものに同じ変換を当てる。ステージしていたパスは、ステージし直す（git rm --cached で
        外したパスは外したまま、git add -N のパスは add -N のまま）
      - 機械で合わせられない worktree（対応表に無いコミット、部分のステージ、衝突やマージ、cherry-pick、revert、
        rebase、bisect の途中、assume-unchanged か skip-worktree の印のあるパス、index.lock を持たれた index、
        パスの増減を含む書き換え、変換か remap が止まるファイル）は、何も変えずに LEFT に出す。書く前にすべてを
        確かめ、remap を最初の書き込みにする（remap は止まるとき何も書かない）

語の指定:
  --dictionary FILE   辞書（prh の形）。省くと nixfiles の ja-terms の辞書（ADR-0014）。重ねて渡せる
  --extra-rules FILE  辞書に足す規則（prh の形。辞書に無い 1 回限りの置き換え）。重ねて渡せる
  --exclude GLOB      直さないパス（リポジトリの直下から。PurePosixPath.full_match の書き方で、** が
                      ディレクトリをまたぐ）。重ねて渡せる。コミットとタグの本文には当てない

run の出力（KEY: value）:
  ORIGINAL: <ミラーのパス>
  REWRITTEN: <書き換えたリポジトリのパス>
  COMMITS: <第 1 段で流したコミットの数>
  BLOBS: <語を直したブロブの数>
  MESSAGES: <語を直したコミットとタグの本文の数>
  NOT_UTF8: <UTF-8 として読めないので直さなかったテキストのブロブの数>
  （ここに commit-refs rewrite-history の出力が続く）
  MAP: <元の番号から最後の番号への対応表のパス>
transform の出力:
  SKIPPED: <パス> (<理由>)  読まなかったファイルごと（symlink、excluded、binary、not UTF-8）
  REPLACED: <パス>    直したファイルごと
  FILES: <直したファイルの数>
sync-worktrees の出力（worktree ごと）:
  RESET: <パス>              変更が無く、新しい HEAD に合わせた
  KEPT: <パス> <パッチ>      変更を残して合わせた。`git -C <パス> diff HEAD` を控えのパッチと比べる
  （ここに commit-refs remap の出力が続く。手を付けていないファイルも挙がるが、その後に新しい中身にする）
  LEFT: <パス> (<理由>)      何も変えなかった。人が見て合わせる
  SKIPPED: <パス> (bare)     作業ツリーが無い
  ERROR: <パス>: <理由>      書き始めた後に止まった（標準エラー）。未コミットの変更は作業ツリーに残るが、HEAD、
                             index、作業ツリーの一部だけが新しい履歴になっている

終了コード: 0 = 成功、1 = 止めたか失敗（sync-worktrees は LEFT か ERROR が 1 つでもあれば）、2 = 使い方の誤り
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath

import prh_rules

# git の buffer_is_binary（xdiff-interface.c の FIRST_FEW_BYTES）と同じ、テキストかどうかを見る先頭のバイト数。
# commit-refs rewrite-history（ADR-0013）も同じ判定を使う
FIRST_FEW_BYTES = 8000
MODE_LINK = b"120000"
MODE_GITLINK = b"160000"
# 照合で比べない、コミットの見出し。tree と parent は別に比べ、署名は書き換えで外れる
SKIPPED_HEADERS = {b"tree", b"parent", b"gpgsig", b"gpgsig-sha256", b"mergetag"}


class Stop(Exception):
    """止める理由。run は REPO に何も書かずに終わる"""


# エスケープした |（GFM ではセルの中の文字。コードスパンの中でも、エスケープしていない | は区切り）
_ESCAPED_PIPE = re.compile(r"\\\|")


@dataclass
class Transformer:
    """第 1 段の変換。ブロブと本文に辞書の規則を当て、直した Markdown の表を揃え直す"""

    rules: list[prh_rules.Rule]
    excludes: list[str]
    table_format: object
    not_utf8: int = 0

    def excluded(self, path: str) -> bool:
        return any(PurePosixPath(path).full_match(glob) for glob in self.excludes)

    def skipped(self, path: bytes, data: bytes) -> str | None:
        """ブロブを読まない理由（excluded、binary、not UTF-8）。読むなら None"""
        if self.excluded(path.decode("utf-8", "surrogateescape")):
            return "excluded"
        if b"\0" in data[:FIRST_FEW_BYTES]:
            return "binary"
        try:
            data.decode("utf-8")
        except UnicodeDecodeError:
            return "not UTF-8"
        return None

    def blob(self, path: bytes, data: bytes) -> bytes | None:
        """パス path のブロブの新しい中身。変えないなら None"""
        reason = self.skipped(path, data)
        if reason == "not UTF-8":
            self.not_utf8 += 1
        if reason is not None:
            return None
        name = path.decode("utf-8", "surrogateescape")
        text = data.decode("utf-8")
        new = self._text(name, text, markdown=name.lower().endswith(".md"))
        return None if new == text else new.encode("utf-8")

    def message(self, data: bytes, name: str) -> bytes:
        """コミットかタグ name の本文。本文は Markdown として表も揃える"""
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError as e:
            raise Stop(f"{name}: UTF-8 として読めない本文がある: {e}") from None
        return self._text(name, text, markdown=True).encode("utf-8")

    def message_would_change(self, data: bytes, encoding: bytes, name: str) -> bool:
        """宣言した符号化 encoding（UTF-8 でない）の、コミット name の本文に、直す語があるか"""
        codec = encoding.decode("ascii", "replace")
        try:
            text = data.decode(codec)
        except LookupError:
            raise Stop(f"{name}: 符号化 {codec} を Python が知らない") from None
        except UnicodeDecodeError as e:
            raise Stop(
                f"{name}: 符号化 {codec} として読めない本文がある: {e}"
            ) from None
        return self._text(name, text, markdown=True) != text

    def _text(self, name: str, text: str, markdown: bool) -> str:
        try:
            new = prh_rules.apply(self.rules, text)
        except prh_rules.TextError as e:
            raise Stop(f"{name}: {e}") from None
        if markdown and new != text:
            new = self._realign_tables(name, text, new)
        return new

    def _realign_tables(self, name: str, old: str, new: str) -> str:
        old_lines, lines = old.split("\n"), new.split("\n")
        if len(old_lines) != len(lines):
            # 行の対応が取れないと、どの表を直したか決まらない
            raise Stop(
                f"{name}: 置き換えで行の数が変わる（表の揃え直しは、行の数を変えない置き換えだけを扱う）"
            )
        changed = {k for k, (a, b) in enumerate(zip(old_lines, lines)) if a != b}
        fml = self.table_format
        # fix-markdown-lint.py の fix_markdown の表の段と同じく、コードフェンスの中を飛ばし、区切りの行が続く
        # 行から表とみなす
        i = 0
        while i < len(lines):
            if fml.fence_open(lines[i]) is not None:
                i = fml.fence_end(lines, i) + 1
                continue
            if not (
                fml.is_table_row(lines[i])
                and i + 1 < len(lines)
                and fml.is_separator_row(lines[i + 1])
            ):
                i += 1
                continue
            j = i
            while j < len(lines) and fml.is_table_row(lines[j]):
                j += 1
            # 直した行を含む表だけを揃え直す（ほかの表の崩れは、この書き換えの範囲の外）
            if changed & set(range(i, j)):
                # format_table は \| でも列を切り、fix_markdown は表の字下げを外す（nixfiles の issue #428）ので、
                # hook と同じ形に揃え直せない表は止める
                for row in lines[i:j]:
                    if _ESCAPED_PIPE.search(row):
                        raise Stop(
                            f"{name}: 直す表の行に \\| がある（揃え直せない）: {row.strip()}"
                        )
                    if row[:1].isspace():
                        raise Stop(
                            f"{name}: 直す表が字下げしてある（揃え直せない）: {row.strip()}"
                        )
                # format_table は行の両端の空白とともに CRLF の CR も外すので、元の行の CR を戻す
                crs = ["\r" if row.endswith("\r") else "" for row in lines[i:j]]
                rows = fml.format_table(lines[i:j])
                lines[i:j] = [row + cr for row, cr in zip(rows, crs)]
            i = j
        return "\n".join(lines)


def load_table_format():
    """nixfiles の fix-markdown-lint.py（package.nix が store のパスを渡す）を読み込む"""
    path = os.environ.get("HISTORY_REWRITE_FIX_MARKDOWN_LINT")
    if not path:
        raise Stop(
            "HISTORY_REWRITE_FIX_MARKDOWN_LINT が無い（package.nix のラッパーから呼ぶ）"
        )
    spec = importlib.util.spec_from_file_location("fix_markdown_lint", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def make_transformer(args: argparse.Namespace) -> Transformer:
    dictionaries = args.dictionary
    if not dictionaries:
        default = os.environ.get("HISTORY_REWRITE_DICTIONARY")
        if not default:
            raise Stop("--dictionary も HISTORY_REWRITE_DICTIONARY も無い")
        dictionaries = [default]
    paths = [Path(p) for p in dictionaries + args.extra_rules]
    try:
        rules = prh_rules.load(paths)
    except prh_rules.DictionaryError as e:
        raise Stop(str(e)) from None
    return Transformer(rules, args.exclude, load_table_format())


def git(
    repo: Path, *args: str, env: dict | None = None, input: bytes | None = None
) -> bytes:
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        env=env,
        input=input,
        check=False,
    )
    if result.returncode != 0:
        raise Stop(
            f"git {' '.join(args)}（終了コード {result.returncode}）: "
            f"{result.stderr.decode('utf-8', 'replace').strip()}"
        )
    return result.stdout


class CatFile:
    """`git cat-file --batch` で、オブジェクトの種類と中身を読む"""

    def __init__(self, repo: Path, env: dict):
        self._proc = subprocess.Popen(
            ["git", "-C", str(repo), "cat-file", "--batch"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            env=env,
        )

    def read(self, oid: bytes) -> tuple[bytes, bytes]:
        self._proc.stdin.write(oid + b"\n")
        self._proc.stdin.flush()
        header = self._proc.stdout.readline()
        parts = header.split()
        if len(parts) != 3:
            raise Stop(
                f"オブジェクト {oid.decode()} を読めない: {header.decode().strip()}"
            )
        size = int(parts[2])
        data = self._proc.stdout.read(size + 1)[:-1]
        return parts[1], data

    def blob(self, oid: bytes) -> bytes:
        kind, data = self.read(oid)
        if kind != b"blob":
            raise Stop(f"{oid.decode()} がブロブでない: {kind.decode()}")
        return data

    def close(self) -> None:
        self._proc.stdin.close()
        self._proc.stdout.close()
        self._proc.wait()


def read_map(path: Path) -> dict[str, str]:
    """filter-repo の commit-map（`old new` の見出しと、1 行に旧と新）"""
    mapping: dict[str, str] = {}
    lines = path.read_text().splitlines()
    rows = lines[1:] if lines and lines[0].split() == ["old", "new"] else lines
    for number, line in enumerate(rows, start=2 if rows is not lines else 1):
        fields = line.split()
        if not fields:
            continue
        if len(fields) != 2:
            raise Stop(f"{path}:{number}: 2 列でない: {line!r}")
        mapping[fields[0]] = fields[1]
    return mapping


# 署名の扱い、cat-file --batch の読み方、対応表の形は commit-refs（modules/home/parts/commit-refs/src/history.rs）と
# 対にして変える（言語が違うので共有できない）。
# git の gpg-interface.c の署名の開始マーカー。filter-repo は fast-export に --signed-tags=strip を渡し、タグの本文の
# 最後のマーカーの行から後を外す（parse_signed_buffer）
SIGNATURE_MARKERS = (
    b"-----BEGIN PGP SIGNATURE-----",
    b"-----BEGIN PGP MESSAGE-----",
    b"-----BEGIN SIGNED MESSAGE-----",
    b"-----BEGIN SSH SIGNATURE-----",
)


def strip_signature(body: bytes) -> bytes:
    """タグの本文から署名を外す（parse_signed_buffer と同じく、マーカーで始まる最後の行から後）"""
    start = 0
    cut = len(body)
    while start < len(body):
        end = body.find(b"\n", start)
        end = len(body) if end < 0 else end + 1
        if body.startswith(SIGNATURE_MARKERS, start):
            cut = start
        start = end
    return body[:cut]


def split_object(data: bytes) -> tuple[list[tuple[bytes, bytes]], bytes]:
    """コミットかタグのオブジェクトを、見出し（続きの行を含む）と本文に分ける"""
    head, sep, body = data.partition(b"\n\n")
    if not sep:
        head, body = data.rstrip(b"\n"), b""
    headers: list[tuple[bytes, bytes]] = []
    for line in head.split(b"\n"):
        if line.startswith(b" ") and headers:
            key, value = headers[-1]
            headers[-1] = (key, value + b"\n" + line)
        else:
            key, _, value = line.partition(b" ")
            headers.append((key, value))
    return headers, body


@dataclass
class Verifier:
    """元の履歴（ミラーのオブジェクトを alternates で見る）と第 1 段の結果を突き合わせる"""

    rewritten: Path
    original: Path
    mapping: dict[str, str]
    transformer: Transformer
    problems: list[str] = field(default_factory=list)
    expected: dict[tuple[bytes, bytes], bytes] = field(default_factory=dict)
    verified_trees: set[tuple[bytes, bytes]] = field(default_factory=set)

    def run(self) -> None:
        env = dict(os.environ)
        env["GIT_ALTERNATE_OBJECT_DIRECTORIES"] = str(self.original / "objects")
        self.env = env
        self.cat = CatFile(self.rewritten, env)
        fmt = git(self.rewritten, "rev-parse", "--show-object-format").decode().strip()
        if fmt not in ("sha1", "sha256"):
            raise Stop(f"オブジェクトの形式 {fmt} を扱わない")
        self.hash = {"sha1": hashlib.sha1, "sha256": hashlib.sha256}[fmt]
        try:
            originals = (
                # filter-repo に渡すのはブランチとタグだけなので、ミラーに写った refs/replace/ には従わない
                git(
                    self.original,
                    "--no-replace-objects",
                    "rev-list",
                    "--branches",
                    "--tags",
                )
                .decode()
                .split()
            )
            missing = sorted(set(originals) - self.mapping.keys())
            if missing:
                self.problems.append(
                    f"commit-map に無い元のコミット: {' '.join(missing)}"
                )
            pruned = {old for old, new in self.mapping.items() if set(new) == {"0"}}
            if pruned:
                self.problems.append(
                    f"第 1 段でコミットが消えた: {' '.join(sorted(pruned))}"
                )
            # 番号の変わらないコミットも確かめる（変換が中身を変えるはずなのに filter-repo が当てなかったものを見る）
            for old, new in self.mapping.items():
                if old not in pruned:
                    self.commit(old, new)
            self.refs()
        finally:
            self.cat.close()
        if self.problems:
            shown = "\n".join(self.problems[:50])
            raise Stop(f"照合が合わない（{len(self.problems)} 件）:\n{shown}")

    def commit(self, old: str, new: str) -> None:
        _, old_data = self.cat.read(old.encode())
        _, new_data = self.cat.read(new.encode())
        old_headers, old_body = split_object(old_data)
        new_headers, new_body = split_object(new_data)

        def kept(headers):
            return [(k, v) for k, v in headers if k not in SKIPPED_HEADERS]

        if kept(old_headers) != kept(new_headers):
            differ = sorted(
                {k.decode() for k, _ in set(kept(old_headers)) ^ set(kept(new_headers))}
            )
            # fast-export は知らない見出し（jj の change-id など）を書き出さないので、その名前を示す
            self.problems.append(f"{old}: 見出しが変わった: {', '.join(differ)}")
        old_parents = [v.decode() for k, v in old_headers if k == b"parent"]
        new_parents = [v.decode() for k, v in new_headers if k == b"parent"]
        if [self.mapping.get(p, p) for p in old_parents] != new_parents:
            self.problems.append(
                f"{old}: 親が対応しない: {old_parents} → {new_parents}"
            )
        # 宣言した符号化が UTF-8 でない本文は変えない（変わるなら第 1 段が止める）
        old_fields = dict(old_headers)
        utf8 = is_utf8(old_fields.get(b"encoding"))
        want = (
            self.transformer.message(old_body, f"コミット {old} の本文")
            if utf8
            else old_body
        )
        if want != new_body:
            self.problems.append(f"{old}: 本文が変換の結果と違う")
        trees = (old_fields[b"tree"], dict(new_headers)[b"tree"])
        # 同じ tree の組は履歴の中で何度も出る（語を直さないコミット、マージ）ので、1 度だけ比べる
        if trees not in self.verified_trees:
            self.tree(old, new)
            self.verified_trees.add(trees)

    def tree(self, old: str, new: str) -> None:
        old_tree, new_tree = self.ls_tree(old), self.ls_tree(new)
        for path in old_tree.keys() - new_tree.keys():
            self.problems.append(f"{old}: {path!r} が消えた")
        for path in new_tree.keys() - old_tree.keys():
            self.problems.append(f"{old}: {path!r} が増えた")
        for path in old_tree.keys() & new_tree.keys():
            (old_mode, old_kind, old_oid), (new_mode, _, new_oid) = (
                old_tree[path],
                new_tree[path],
            )
            if old_mode != new_mode:
                self.problems.append(f"{old}: {path!r} のモードが変わった")
                continue
            want = old_oid
            if old_kind == b"blob" and old_mode not in (MODE_LINK, MODE_GITLINK):
                want = self.expected_oid(path, old_oid)
            if want != new_oid:
                self.problems.append(f"{old}: {path!r} が変換の結果と違う")

    def expected_oid(self, path: bytes, oid: bytes) -> bytes:
        """パス path のブロブ oid に変換を当てた中身の番号。同じブロブは履歴の中で何度も出るので、番号だけを覚える"""
        key = (path, oid)
        if key not in self.expected:
            content = self.transformer.blob(path, self.cat.blob(oid))
            self.expected[key] = oid if content is None else self.blob_oid(content)
        return self.expected[key]

    def ls_tree(self, commit: str) -> dict[bytes, tuple[bytes, bytes, bytes]]:
        out = git(
            self.rewritten, "ls-tree", "-r", "-z", "--full-tree", commit, env=self.env
        )
        entries = {}
        for record in out.split(b"\0"):
            if record:
                meta, path = record.split(b"\t", 1)
                mode, kind, oid = meta.split()
                entries[path] = (mode, kind, oid)
        return entries

    def blob_oid(self, content: bytes) -> bytes:
        return self.hash(b"blob %d\0" % len(content) + content).hexdigest().encode()

    def refs(self) -> None:
        old_refs = self.for_each_ref(self.original)
        new_refs = self.for_each_ref(self.rewritten)
        if old_refs.keys() != new_refs.keys():
            self.problems.append(
                f"ブランチとタグの名前が合わない: {sorted(old_refs.keys() ^ new_refs.keys())}"
            )
        for name in old_refs.keys() & new_refs.keys():
            (old_oid, kind), (new_oid, _) = old_refs[name], new_refs[name]
            if kind == "commit":
                if self.mapping.get(old_oid) != new_oid:
                    self.problems.append(f"{name}: 対応表の新を指さない")
            elif kind == "tag":
                if self.tag(old_oid) != new_oid:
                    self.problems.append(f"{name}: 注釈つきのタグが変換の結果と違う")
            else:
                self.problems.append(f"{name}: コミットとタグのほか（{kind}）を指す")

    def tag(self, oid: str) -> str:
        """注釈つきのタグを、第 1 段が作るはずのオブジェクトにしたときの番号"""
        _, data = self.cat.read(oid.encode())
        headers, body = split_object(data)
        rebuilt = []
        if b"tagger" not in dict(headers):
            # filter-repo は fast-export に --fake-missing-tagger を渡す（仮の tagger の値は fast-export.c のもの。git 2.56.0 で確かめた）
            at = [k for k, _ in headers].index(b"tag") + 1
            headers = (
                headers[:at]
                + [(b"tagger", b"Unspecified Tagger <unspecified-tagger> 0 +0000")]
                + headers[at:]
            )
        for key, value in headers:
            if key == b"object":
                target = value.decode()
                if dict(headers)[b"type"] == b"tag":
                    raise Stop(f"{oid}: タグを指すタグ（run の初めに止めるはずのもの）")
                value = self.mapping.get(target, target).encode()
            rebuilt.append(b"%s %s" % (key, value))
        message = self.transformer.message(strip_signature(body), f"タグ {oid} の本文")
        content = b"\n".join(rebuilt) + b"\n\n" + message
        return self.hash(b"tag %d\0" % len(content) + content).hexdigest()

    def for_each_ref(self, repo: Path) -> dict[str, tuple[str, str]]:
        out = git(
            repo,
            "for-each-ref",
            "--format=%(refname) %(objectname) %(objecttype)",
            "refs/heads/",
            "refs/tags/",
        )
        refs = {}
        for line in out.decode().splitlines():
            name, oid, kind = line.split(" ")
            refs[name] = (oid, kind)
        return refs


def refuse_nested_tags(repo: Path) -> None:
    """タグを指すタグがあれば止める（fast-export は外のタグの名前で内のタグを作り直すので、照合できる形でない）"""
    out = git(
        repo, "for-each-ref", "--format=%(refname) %(objecttype) %(type)", "refs/tags/"
    )
    nested = [
        line.split()[0]
        for line in out.decode().splitlines()
        if line.split()[1:] == ["tag", "tag"]
    ]
    if nested:
        hint = "、".join(
            f"git tag -f {n.removeprefix('refs/tags/')} {n.removeprefix('refs/tags/')}^{{}}"
            for n in nested
        )
        raise Stop(
            f"タグを指すタグがある: {' '.join(nested)}（コミットを指すタグに直してから: {hint}）"
        )


def is_utf8(encoding: bytes | None) -> bool:
    """コミットの encoding の見出しが無いか UTF-8 か（git の is_encoding_utf8 と同じく、大文字と小文字、- を問わない）"""
    return encoding is None or encoding.lower().replace(b"-", b"") == b"utf8"


def stage1(repo: Path, transformer: Transformer) -> dict[str, int]:
    """第 1 段: filter-repo に語の変換だけを流す"""
    import git_filter_repo as fr

    counts = {"blobs": 0, "messages": 0}
    seen: dict[tuple[bytes, bytes], bytes] = {}

    def file_info(filename, mode, blob_id, value):
        if mode in (MODE_LINK, MODE_GITLINK):
            return (filename, mode, blob_id)
        key = (filename, blob_id)
        if key not in seen:
            new = transformer.blob(filename, value.get_contents_by_identifier(blob_id))
            if new is None:
                seen[key] = blob_id
            else:
                seen[key] = value.insert_file_with_contents(new)
                counts["blobs"] += 1
        return (filename, mode, seen[key])

    def commit(commit, _metadata):
        name = f"コミット {commit.original_id.decode()} の本文"
        if not is_utf8(commit.encoding):
            # 本文は宣言した符号化で読まれる。UTF-8 の語を書き込むと文字化けするので、変えずに止める
            if transformer.message_would_change(commit.message, commit.encoding, name):
                raise Stop(
                    f"{commit.original_id.decode()}: 符号化 {commit.encoding.decode()} の本文に直す語がある"
                    "（UTF-8 の本文だけを直す）"
                )
            return
        new = transformer.message(commit.message, name)
        if new != commit.message:
            counts["messages"] += 1
            commit.message = new

    def tag(tag, _metadata):
        new = transformer.message(tag.message, f"タグ {tag.ref.decode()} の本文")
        if new != tag.message:
            counts["messages"] += 1
            tag.message = new

    # 1 対 1 で照合するため、空になったコミットも消さない（語を手で直しただけのコミットは、変換の後に空になる）。
    # 本文の番号は第 2 段が付け直すので、filter-repo には付け直させない
    args = fr.FilteringOptions.parse_args(
        [
            "--force",
            "--quiet",
            "--prune-empty=never",
            "--prune-degenerate=never",
            "--preserve-commit-hashes",
            "--replace-refs=delete-no-add",
            # 本文を元の符号化のまま渡す（UTF-8 に直すと encoding の見出しが消え、照合が合わない）
            "--preserve-commit-encoding",
            # 第 2 段がすぐに全体を書き直すので、ここで repack しない
            "--no-gc",
        ]
    )
    cwd = os.getcwd()
    os.chdir(repo)
    try:
        fr.RepoFilter(
            args, file_info_callback=file_info, commit_callback=commit, tag_callback=tag
        ).run()
    finally:
        os.chdir(cwd)
    return counts


def compose(first: dict[str, str], second: dict[str, str]) -> dict[str, str]:
    return {old: second.get(mid, mid) for old, mid in first.items()}


def run(args: argparse.Namespace) -> None:
    transformer = make_transformer(args)
    source = Path(args.source).resolve()
    workdir = Path(args.workdir).resolve()
    if workdir == source or workdir.is_relative_to(source):
        raise Stop(f"{workdir} が {source} の中（元のリポジトリの外に置く）")
    if workdir.exists() and not workdir.is_dir():
        raise Stop(f"{workdir} がディレクトリでない")
    if workdir.exists() and any(workdir.iterdir()):
        raise Stop(f"{workdir} が空でない（前の書き換えを確かめて片付けてから）")
    workdir.mkdir(parents=True, exist_ok=True)
    original = workdir / "original.git"
    rewritten = workdir / "rewritten"

    git(workdir, "clone", "-q", "--mirror", str(source), str(original))
    head = subprocess.run(
        ["git", "-C", str(original), "symbolic-ref", "-q", "HEAD"],
        capture_output=True,
        check=False,
    )
    if head.returncode != 0:
        raise Stop(f"{source} の HEAD がブランチを指していない（ブランチに移ってから）")
    head = head.stdout.decode().strip()
    if not git(original, "for-each-ref", head):
        raise Stop(f"{source} の HEAD のブランチ {head} にコミットが無い")
    git(workdir, "init", "-q", str(rewritten))
    git(
        rewritten,
        "fetch",
        "-q",
        "--no-tags",
        # 作ったばかりのリポジトリの HEAD（まだコミットの無いブランチ）にも取り込む
        "--update-head-ok",
        str(original),
        "+refs/heads/*:refs/heads/*",
        "+refs/tags/*:refs/tags/*",
    )
    git(rewritten, "symbolic-ref", "HEAD", head)
    git(rewritten, "reset", "-q", "--hard")
    refuse_nested_tags(rewritten)

    counts = stage1(rewritten, transformer)
    # 照合も同じブロブを読み直して数えるので、第 1 段の数をここで取る
    not_utf8 = transformer.not_utf8
    stage1_map = read_map(rewritten / ".git" / "filter-repo" / "commit-map")
    Verifier(rewritten, original, stage1_map, transformer).run()

    print(f"ORIGINAL: {original}")
    print(f"REWRITTEN: {rewritten}")
    print(f"COMMITS: {len(stage1_map)}")
    print(f"BLOBS: {counts['blobs']}")
    print(f"MESSAGES: {counts['messages']}")
    print(f"NOT_UTF8: {not_utf8}", flush=True)

    result = subprocess.run(
        [
            "commit-refs",
            "rewrite-history",
            str(rewritten / ".git" / "filter-repo" / "commit-map"),
        ],
        cwd=rewritten,
        check=False,
    )
    if result.returncode != 0:
        raise Stop("commit-refs rewrite-history が止まった（理由は上）")
    final = compose(
        stage1_map, read_map(rewritten / ".git" / "commit-refs" / "commit-map")
    )
    map_path = workdir / "commit-map"
    map_path.write_text(
        "old new\n" + "".join(f"{o} {n}\n" for o, n in sorted(final.items()))
    )
    print(f"MAP: {map_path}")


def transform_files(args: argparse.Namespace) -> None:
    transformer = make_transformer(args)
    top = Path(git(Path("."), "rev-parse", "--show-toplevel").decode().strip())
    # すべてのファイルの新しい中身を作ってから書く（途中で止まるときは、どのファイルにも書かない）
    results: list[tuple[Path, str, bytes]] = []
    skipped: list[tuple[str, str]] = []
    for name in args.files:
        # symlink のファイルそのものは見分けたいので、親のディレクトリだけをたどる（top は実体のパス）
        given = Path(name).absolute()
        path = given.parent.resolve() / given.name
        try:
            relative = path.relative_to(top).as_posix()
        except ValueError:
            raise Stop(f"{name}: リポジトリ {top} の外") from None
        if path.is_symlink():
            # 履歴の書き換えと同じく、symlink は変えない（先のファイルは、それ自身のパスで直す）
            skipped.append((relative, "symlink"))
            continue
        if not path.is_file():
            raise Stop(f"{name}: ファイルでない（無いか、ディレクトリ）")
        data = path.read_bytes()
        reason = transformer.skipped(relative.encode(), data)
        if reason is not None:
            skipped.append((relative, reason))
            continue
        new = transformer.blob(relative.encode(), data)
        if new is not None:
            results.append((path, relative, new))
    replaced = []
    for path, relative, new in results:
        path.write_bytes(new)
        replaced.append(relative)
    for relative, reason in skipped:
        print(f"SKIPPED: {relative} ({reason})")
    for relative in replaced:
        print(f"REPLACED: {relative}")
    print(f"FILES: {len(replaced)}")


class Leave(Exception):
    """この worktree は何も変えずに残す理由（人が見て合わせる）"""


def _entries(raw: bytes) -> dict[bytes, bytes]:
    """`ls-files -s -z` と `ls-tree -r -z` の行を、パスから「モードと番号」への表にする"""
    entries = {}
    for line in raw.split(b"\0"):
        if not line:
            continue
        head, path = line.split(b"\t", 1)
        fields = head.split()
        # ls-files -s は「モード 番号 段」、ls-tree は「モード 種類 番号」
        if len(fields) == 3 and fields[1] in (b"blob", b"commit", b"tree"):
            mode, oid = fields[0], fields[2]
        else:
            if fields[2] != b"0":
                raise Leave(f"衝突の途中（{path.decode(errors='replace')}）")
            mode, oid = fields[0], fields[1]
        entries[path] = mode + b" " + oid
    return entries


def _rewritten_paths(w: Path, old: str, new: str) -> list[bytes]:
    """old から new で中身の変わったパス。パスの増減と種類の変わったものがあれば残す"""
    raw = git(w, "diff-tree", "-r", "-z", "--no-renames", old, new)
    fields = raw.split(b"\0")
    paths = []
    for status_line, path in zip(fields[0::2], fields[1::2]):
        mode_old, mode_new, _, _, status = status_line.lstrip(b":").split()
        if status != b"M" or mode_old != mode_new:
            raise Leave(
                f"書き換えが {path.decode(errors='replace')} を {status.decode()} にした"
            )
        paths.append(path)
    return paths


def _names(raw: bytes) -> set[bytes]:
    """`-z --name-only` の出力の、パスの集まり"""
    return {p for p in raw.split(b"\0") if p}


def _paths_input(paths) -> bytes:
    return b"".join(p + b"\0" for p in paths)


# 進行中の操作の印（rev-parse --git-path で worktree ごとの場所を引く）。残したまま HEAD を移すと、印が古い履歴の
# コミットを指したまま、次のコミットやマージが古い履歴を親に持ち込む
_OPERATIONS = (
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "REBASE_HEAD",
    "rebase-merge",
    "rebase-apply",
    "sequencer",
    "BISECT_LOG",
)


@dataclass
class _Plan:
    """ref を移す前の HEAD と比べた、1 つの worktree の未コミットの変更（読むだけで作る）"""

    old: str
    new: str
    move_head: bool  # detached HEAD を old から new に移す
    rewritten: list[bytes]
    index: dict[bytes, bytes]
    staged: set[bytes]
    unstaged: set[bytes]


def _plan(w: Path, old: str, mapping: dict[str, str]) -> _Plan:
    """合わせ方を決める。合わせられなければ Leave を投げる。worktree には index の stat の更新だけを書く"""
    new = mapping.get(old)
    if new is None:
        raise Leave(f"{old} が書き換えに入っていない（1 でブランチを付けてやり直す）")
    head = git(w, "rev-parse", "HEAD").decode().strip()
    detached = (
        subprocess.run(
            ["git", "-C", str(w), "symbolic-ref", "-q", "HEAD"],
            capture_output=True,
            check=False,
        ).returncode
        != 0
    )
    if head != new and not (detached and head == old):
        raise Leave(f"HEAD が {new} でない（ref を移してから）")
    marks = git(w, "rev-parse", *(a for n in _OPERATIONS for a in ("--git-path", n)))
    for name, mark in zip(_OPERATIONS, marks.decode().splitlines()):
        if os.path.lexists(w / mark):
            raise Leave(f"操作の途中（{name}）")
    rewritten = _rewritten_paths(w, old, new)

    # 衝突の項は先に見る（refresh は衝突の項があると needs merge で失敗し、理由を取り違える）
    index = _entries(git(w, "ls-files", "-s", "-z"))
    tree = _entries(git(w, "ls-tree", "-r", "-z", "--full-tree", old))

    # 未コミットの変更は、ref を移す前の HEAD（old）と比べる。index の stat を更新してから作業ツリーと比べる。
    # 更新できない（index.lock を持たれている）なら、後の index の書き込みも失敗するので、何も変えずに残す
    refresh = subprocess.run(
        ["git", "-C", str(w), "update-index", "-q", "--refresh"],
        capture_output=True,
        check=False,
    )
    if refresh.returncode != 0:
        reason = refresh.stderr.decode("utf-8", "replace").strip()
        raise Leave(
            f"index を更新できない（終了コード {refresh.returncode}。index.lock を持つプロセスが"
            f"無いか確かめる）{reason}"
        )
    # assume-unchanged と skip-worktree の印のあるパスは、diff-files が作業ツリーを見ないので、手元の編集を見落とす
    # （ls-files -v の小文字の印と S）
    flagged = [
        line[2:].decode(errors="replace")
        for line in git(w, "ls-files", "-v", "-z").split(b"\0")
        if line and (line[:1].islower() or line[:1] == b"S")
    ]
    if flagged:
        raise Leave(
            f"assume-unchanged か skip-worktree の印のあるパス: {', '.join(flagged)}"
        )
    unstaged = _names(git(w, "diff-files", "-z", "--name-only"))
    # git add -N のパスは、index に空の項があるだけで中身はステージしていない。--ita-invisible-in-index は
    # その項を無いものとして比べる（diff-files の状態は、ファイルを消すと A から D に変わるので、見分けに使えない）
    staged = _names(
        git(
            w,
            "diff-index",
            "--cached",
            "--no-renames",
            "--ita-invisible-in-index",
            "-z",
            "--name-only",
            old,
        )
    )
    intent = (
        _names(
            git(w, "diff-index", "--cached", "--no-renames", "-z", "--name-only", old)
        )
        - staged
    )
    if intent & tree.keys():
        names = ", ".join(
            sorted(p.decode(errors="replace") for p in intent & tree.keys())
        )
        raise Leave(f"追跡していたパスを git add -N で足し直した: {names}")
    partly = sorted(staged & unstaged)
    if partly:
        # ステージし直すと、ステージしていなかった部分も入る
        names = ", ".join(p.decode(errors="replace") for p in partly)
        raise Leave(f"ステージした後に変えたパス（部分のステージ）: {names}")
    return _Plan(old, new, detached and head == old, rewritten, index, staged, unstaged)


def _converted(w: Path, plan: _Plan, t: Transformer) -> list[tuple[Path, bytes]]:
    """手を付けた、追跡している書き換えたファイルの、変換を当てた中身。変換が止まれば Stop を投げる"""
    contents = []
    for p in plan.rewritten:
        if p not in plan.staged and p not in plan.unstaged:
            continue
        # index に無いパス（git rm --cached、git rm）と、未コミットで消したファイルは、そのまま残す
        f = w / os.fsdecode(p)
        if p not in plan.index or (not f.exists() and not f.is_symlink()):
            continue
        if f.is_symlink() or not f.is_file():
            raise Leave(f"{p.decode(errors='replace')} がファイルでなくなった")
        try:
            changed = t.blob(p, f.read_bytes())
        except Stop as e:
            raise Stop(f"{p.decode(errors='replace')} の変換が止まった: {e}") from None
        if changed is not None:
            contents.append((f, changed))
    return contents


def sync_one(
    w: Path, old: str, mapping: dict[str, str], map_path: Path, t: Transformer
) -> str | None:
    """1 つの worktree を合わせる。変更が無ければ None、あれば commit-refs remap の出力を返す。
    書く前に止まれば、何も変えずに Leave を投げる。書き始めた後に止まれば、worktree のパスを添えて Stop を投げる"""
    try:
        plan = _plan(w, old, mapping)
        clean = not plan.staged and not plan.unstaged
        if not clean:
            _converted(w, plan, t)  # 書く前に、変換が止まらないことを確かめる
            # 未コミットの文書の古い番号を付け直す。これを最初の書き込みにするのは、remap が止まる理由（曖昧な番号、
            # 消えたコミットを指す番号）は文書の中身によって起き、止まるときは何も書かないので、worktree を変えずに残せるため
            remap = subprocess.run(
                ["commit-refs", "remap", str(map_path)],
                cwd=w,
                capture_output=True,
                check=False,
            )
            stderr = remap.stderr.decode("utf-8", "replace").strip()
            if remap.returncode != 0 and stderr.endswith("何も書き換えていない"):
                raise Leave(f"commit-refs remap が止まった: {stderr}")
    except (Stop, OSError) as e:
        raise Leave(str(e)) from None

    try:
        if not clean and remap.returncode != 0:
            raise Stop(f"commit-refs remap が書き込みの途中で止まった: {stderr}")
        return _apply(w, plan, clean, t, b"" if clean else remap.stdout)
    except (Stop, Leave, OSError) as e:
        raise Stop(
            f"{w}: 途中まで合わせて止まった（未コミットの変更は作業ツリーに残る。HEAD、index、"
            f"作業ツリーの一部が新しい履歴になっている）: {e}"
        ) from None


def _apply(w: Path, plan: _Plan, clean: bool, t: Transformer, remapped: bytes):
    # GIT_LITERAL_PATHSPECS を足すのは、パスの *、? を pathspec の型として読ませないため
    env = dict(os.environ, GIT_LITERAL_PATHSPECS="1")
    # 変換は、remap が番号を付け直した後の中身に当てる
    contents = [] if clean else _converted(w, plan, t)
    if plan.move_head:
        git(w, "update-ref", "--no-deref", "HEAD", plan.new, plan.old)
    if clean:
        # 書き換えはパスを増やさないので（_rewritten_paths）、追跡していないファイルを上書きしない
        git(w, "reset", "-q", "--hard", "HEAD", env=env)
        return None
    untouched = [
        p for p in plan.rewritten if p not in plan.staged and p not in plan.unstaged
    ]
    if untouched:
        # 手を付けていないファイルは、index と作業ツリーを新しい中身にする
        git(
            w,
            "checkout",
            "HEAD",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
            env=env,
            input=_paths_input(untouched),
        )
    index_only = [p for p in plan.rewritten if p in plan.unstaged]
    if index_only:
        # 変えたがステージしていないファイルの index を、新しい HEAD の中身にする（作業ツリーは変えない）
        git(
            w,
            "reset",
            "-q",
            "HEAD",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
            env=env,
            input=_paths_input(index_only),
        )
    for f, data in contents:
        _replace_file(f, data)
    # ステージしていたパスを、直した中身でステージし直す（部分のステージは _plan で除いた）。index に無いパス
    # （git rm --cached、git rm）は足さない
    restage = sorted(p for p in plan.staged if p in plan.index)
    if restage:
        git(
            w,
            "add",
            "-f",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
            env=env,
            input=_paths_input(restage),
        )
    return remapped.decode("utf-8", "replace")


def _replace_file(f: Path, data: bytes) -> None:
    """f の中身を data にする。同じディレクトリの一時ファイルに書き、f の権限をコピーして rename で置き換える
    （write_bytes は先に中身を消すので、書き込みの失敗で未コミットの編集が壊れる。読み出し専用のファイルも書ける）"""
    mode = f.stat().st_mode & 0o7777
    fd, tmp = tempfile.mkstemp(dir=f.parent, prefix=f".{f.name}.")
    try:
        with os.fdopen(fd, "wb") as out:
            out.write(data)
        os.chmod(tmp, mode)
        os.replace(tmp, f)
    except BaseException:
        os.unlink(tmp)
        raise


def _one_line(e: Exception) -> str:
    """git と remap の複数行の理由を、worktree ごとの 1 行の出力に収める"""
    return " / ".join(line.strip() for line in str(e).splitlines() if line.strip())


def read_state(path: Path) -> list[tuple[Path, str, str]]:
    rows = []
    for number, line in enumerate(path.read_text().splitlines(), start=1):
        if not line:
            continue
        fields = line.split("\t")
        if len(fields) != 3:
            raise Stop(f"{path}:{number}: タブで区切った 3 列でない: {line!r}")
        rows.append((Path(fields[0]), fields[1], fields[2]))
    return rows


def sync_worktrees(args: argparse.Namespace) -> bool:
    transformer = make_transformer(args)
    workdir = Path(args.workdir).resolve()
    map_path = workdir / "commit-map"
    mapping = read_map(map_path)
    rows = read_state(workdir / "worktree-state")
    ok = True
    for w, old, patch in rows:
        try:
            if not w.is_dir():
                raise Leave("ディレクトリが無い")
            try:
                bare = git(w, "rev-parse", "--is-bare-repository").strip() == b"true"
            except (Stop, OSError) as e:
                raise Leave(str(e)) from None
            if bare:
                print(f"SKIPPED: {w} (bare)")
                continue
            remapped = sync_one(w, old, mapping, map_path, transformer)
        except Leave as e:
            print(f"LEFT: {w} ({_one_line(e)})", flush=True)
            ok = False
            continue
        except Stop as e:
            print(f"ERROR: {_one_line(e)}", file=sys.stderr, flush=True)
            ok = False
            continue
        if remapped is None:
            print(f"RESET: {w}", flush=True)
        else:
            print(f"KEPT: {w} {patch}")
            print(remapped, end="", flush=True)
    return ok


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="history-rewrite")
    words = argparse.ArgumentParser(add_help=False)
    words.add_argument("--dictionary", action="append", default=[])
    words.add_argument("--extra-rules", action="append", default=[])
    words.add_argument("--exclude", action="append", default=[])
    sub = p.add_subparsers(dest="command", required=True)
    r = sub.add_parser("run", parents=[words])
    r.add_argument("--source", required=True)
    r.add_argument("--workdir", required=True)
    t = sub.add_parser("transform", parents=[words])
    t.add_argument("files", nargs="+")
    s = sub.add_parser("sync-worktrees", parents=[words])
    s.add_argument("--workdir", required=True)
    return p


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "run":
            run(args)
        elif args.command == "sync-worktrees":
            if not sync_worktrees(args):
                return 1
        else:
            transform_files(args)
    except Stop as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
