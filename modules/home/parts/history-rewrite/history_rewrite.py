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
  REPLACED: <パス>    直したファイルごと
  FILES: <直したファイルの数>

終了コード: 0 = 成功、1 = 止めたか失敗、2 = 使い方の誤り
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import os
import subprocess
import sys
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


@dataclass
class Transformer:
    """第 1 段の変換。ブロブと本文に辞書の規則を当て、直した Markdown の表を揃え直す"""

    rules: list[prh_rules.Rule]
    excludes: list[str]
    table_format: object
    not_utf8: int = 0
    _cache: dict[tuple[bytes, bytes], bytes | None] = field(default_factory=dict)

    def excluded(self, path: str) -> bool:
        return any(PurePosixPath(path).full_match(glob) for glob in self.excludes)

    def blob(self, path: bytes, data: bytes) -> bytes | None:
        """パス path のブロブの新しい中身。変えないなら None"""
        name = path.decode("utf-8", "surrogateescape")
        if self.excluded(name) or b"\0" in data[:FIRST_FEW_BYTES]:
            return None
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError:
            self.not_utf8 += 1
            return None
        new = self._text(name, text, markdown=name.lower().endswith(".md"))
        return None if new == text else new.encode("utf-8")

    def cached_blob(self, path: bytes, oid: bytes, read) -> bytes | None:
        """blob を、パスとブロブの番号で覚えておく（同じブロブは履歴の中で何度も出る）"""
        key = (path, oid)
        if key not in self._cache:
            self._cache[key] = self.blob(path, read(oid))
        return self._cache[key]

    def message(self, data: bytes) -> bytes:
        """コミットかタグの本文。本文は Markdown として表も揃える"""
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError as e:
            raise Stop(f"UTF-8 として読めない本文がある: {e}") from None
        return self._text("（本文）", text, markdown=True).encode("utf-8")

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
                indent = lines[i][: len(lines[i]) - len(lines[i].lstrip())]
                # format_table は行の両端の空白とともに CRLF の CR も外すので、元の行の CR を戻す
                crs = ["\r" if row.endswith("\r") else "" for row in lines[i:j]]
                rows = fml.format_table(lines[i:j])
                lines[i:j] = [indent + row + cr for row, cr in zip(rows, crs)]
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
    for line in lines[1:] if lines and lines[0].split() == ["old", "new"] else lines:
        if not line.strip():
            continue
        old, new = line.split()
        mapping[old] = new
    return mapping


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

    def run(self) -> None:
        env = dict(os.environ)
        env["GIT_ALTERNATE_OBJECT_DIRECTORIES"] = str(self.original / "objects")
        self.env = env
        self.cat = CatFile(self.rewritten, env)
        fmt = git(self.rewritten, "rev-parse", "--show-object-format").decode().strip()
        self.hash = {"sha1": hashlib.sha1, "sha256": hashlib.sha256}[fmt]
        try:
            pruned = [old for old, new in self.mapping.items() if set(new) == {"0"}]
            if pruned:
                self.problems.append(f"第 1 段でコミットが消えた: {' '.join(pruned)}")
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
            self.problems.append(f"{old}: 作者、コミッター、符号化が変わった")
        old_parents = [v.decode() for k, v in old_headers if k == b"parent"]
        new_parents = [v.decode() for k, v in new_headers if k == b"parent"]
        if [self.mapping.get(p, p) for p in old_parents] != new_parents:
            self.problems.append(
                f"{old}: 親が対応しない: {old_parents} → {new_parents}"
            )
        if self.transformer.message(old_body) != new_body:
            self.problems.append(f"{old}: 本文が変換の結果と違う")
        self.tree(old, new)

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
                content = self.transformer.cached_blob(path, old_oid, self.cat.blob)
                if content is not None:
                    want = self.blob_oid(content)
            if want != new_oid:
                self.problems.append(f"{old}: {path!r} が変換の結果と違う")

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
                kind = dict(headers)[b"type"]
                target = (
                    self.tag(target)
                    if kind == b"tag"
                    else self.mapping.get(target, target)
                )
                value = target.encode()
            rebuilt.append(b"%s %s" % (key, value))
        message = self.transformer.message(strip_signature(body))
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

    def message(data):
        new = transformer.message(data)
        if new != data:
            counts["messages"] += 1
        return new

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
            args, file_info_callback=file_info, message_callback=message
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
    replaced = []
    for name in args.files:
        path = Path(name).absolute()
        if path.is_symlink():
            # 履歴の書き換えと同じく、symlink は変えない（先のファイルは、それ自身のパスで直す）
            continue
        if not path.is_file():
            raise Stop(f"{name}: ファイルでない（無いか、ディレクトリ）")
        try:
            relative = path.relative_to(top).as_posix()
        except ValueError:
            raise Stop(f"{name}: リポジトリ {top} の外") from None
        new = transformer.blob(relative.encode(), path.read_bytes())
        if new is not None:
            path.write_bytes(new)
            replaced.append(relative)
    for relative in replaced:
        print(f"REPLACED: {relative}")
    print(f"FILES: {len(replaced)}")


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
    return p


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "run":
            run(args)
        else:
            transform_files(args)
    except Stop as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
