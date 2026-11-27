---
type: ADR
status: accepted
date: 2026-11-27
requires: [ADR-0084]
supersedes: []
superseded-by:
issues: []
description: 静的な設定（claude-settings.json）の env に PYTHONUTF8=1 を置き、Claude Code のセッションが起動する Python を、すべての機械で UTF-8 モードで動かす
---

# ADR-0106: Claude Code のセッションの env に PYTHONUTF8=1 を置き、セッションの Python を UTF-8 モードで動かす

## 背景

Windows の Python は、標準出力と `open` の既定の符号化に ANSI コードページ（日本語の Windows では cp932）を使う。セッションの
中で Python が韓国語や絵文字を print すると `UnicodeEncodeError: 'cp932' codec can't encode character` で落ち、1 つの
セッションで 4 回起きた。ユーザーは UTF-8 を既定にすることを求め、Linux への影響の説明（UTF-8 ロケールではほぼ変わらず、
C ロケールでも Python 3.7 以降は PEP 538/540 で UTF-8 に切り替わる）を受けて、ハーネスの設定に入れることに同意した
（ユーザー 2026-11-27）。

Windows のユーザーの環境変数（`setx PYTHONUTF8 1`）は、ハーネスの外で設定済み（2026-11-27）。それはこの機械のセッションの外の
Python にも効くが、ほかの機械と新しく入れる機械には入らない。

静的な設定は、`claude-setup settings` が `~/.claude/settings.json` の最上位のキーごとに置き換える（ADR-0084、
`crates/claude-setup/src/settings.rs`）。settings.json の `env` を書くのはこれだけで、ハーネスを取り込む nixfiles は
`env` も `PYTHONUTF8` も書かない（nixfiles `bf8b3677`、2026-11-25 を grep）。

測った値は、Windows 11 Pro 10.0.26100 の Python 3.13.1 で、`PYTHONUTF8=1` と無しで次を実行したもの（2026-11-27）:
`sys.flags.utf8_mode`、`sys.stdout.encoding`、`locale.getpreferredencoding(False)`、`open("t.txt", "w").write("a\n")` の
バイト列、`subprocess.run(["cmd", "/d", "/c", "echo あ"], capture_output=True, text=True)`。

## 検討した案

- **env に `PYTHONUTF8=1`**（採用）: 標準入出力、`open` の既定、`locale.getpreferredencoding(False)`、`subprocess` の
  `text=True` が UTF-8 になる
- **env に `PYTHONIOENCODING=utf-8`**: 標準入出力だけを UTF-8 にし、`open` と `subprocess` の既定は cp932 のまま残す。
  起きた失敗（print）は直り、影響の範囲は狭い。ただし `open` で日本語を書き読みするコードは cp932 のファイルを作り続け、
  UTF-8 を既定にするというユーザーの求めに足りない。Python 3.13.1 の Windows で、この変数だけでは
  `locale.getpreferredencoding(False)` が cp932 のまま（2026-11-27 に測った）
- **書くスクリプトごとに `encoding=` と `sys.stdout.reconfigure` を付ける**: セッションでその場で書く Python は数が多く
  使い捨てで、付け忘れのたびに同じ失敗が起きる
- **Windows の `setx` だけ**: この機械では効くが、ほかの機械と新しく入れる機械に入らず、ハーネスの設定から読めない

## 決定と理由

`claude-settings.json` の `env` に `"PYTHONUTF8": "1"` を置く。セッションが起動する Python の文字の扱いを、OS と機械に
よらず UTF-8 に揃えるため。ユーザーの求めは print の失敗を止めることだけでなく UTF-8 を既定にすることで、`open` の既定まで
変えるのはその範囲に入る。

### 帰結

- Python 3.13.1 の Windows で、`sys.stdout.encoding` と `locale.getpreferredencoding(False)` が `utf-8` になり、韓国語と
  絵文字の print が通る。`open(..., "w")` の改行の変換は変わらず CRLF のまま（`b'a\r\n'`。2026-11-27 に測った）
- cp932 で書く Windows のツール（`cmd /c echo あ` など）の出力を `subprocess.run(..., text=True)` で読むと、
  `UnicodeDecodeError: 'utf-8' codec can't decode byte 0x82` で落ちる（同じ版で 2026-11-27 に測った）。cp932 のファイルを
  `encoding=` なしで読むときも同じ。そうしたコードは `encoding="cp932"` を明示する。このリポジトリに Python のファイルは
  無く（`git ls-files '*.py'` が空、2026-11-27）、hook は Rust なので、ハーネスの中に直す呼び出しは無い
- セッションの中では、`encoding=` を書き忘れたコードも UTF-8 で通る。PYTHONUTF8 の無い環境（CI、ほかの人の機械）では
  cp932 で動くので、配るコードは `encoding=` を明示する。セッションが投げる Slurm のジョブも env を受け継ぐ
  （sbatch の既定の `--export=ALL`）ので、同じスクリプトをセッションの外で動かすと既定の符号化が違いうる
- Python 3.15 からは UTF-8 モードが既定になる（PEP 686）。すべての機械の Python が 3.15 以降になれば、この設定は効果が
  無くなり外せる
- `env` は最上位のキーなので、配備のたびに `~/.claude/settings.json` の `env` 全体がこの値に置き換わる。機械ごとの変数を
  手で `env` に足しても、次の配備で消える。2026-11-27 の時点で、この Windows の機械の `~/.claude/settings.json` に `env` は無い。
  ほかの機械（r995 など）の `env` は確かめていない

### 確認

配備の後、`~/.claude/settings.json` の `env.PYTHONUTF8` が `"1"` であることと、新しいセッションの Python で
`sys.flags.utf8_mode` が 1 になることを確かめる。
