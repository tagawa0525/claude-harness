---
status: proposed
date: 2026-11-12
requires: [ADR-0012, ADR-0047]
supersedes: []
superseded-by:
issues: ["#101"]
---

# ADR-0082: ハーネスの hook とスクリプトを Rust で書き、ネイティブの Windows でも動かす

## 背景

ハーネスを、Linux（NixOS と home-manager）だけでなくネイティブの Windows（WSL を使わない）でも使いたい（2026-11-12 の
ユーザーの要望）。Windows で特に要るのは hook（Claude Code の PreToolUse hook と git hook）で、ハーネスの規則を機械的に
守らせる中心になっている。

今の実装の言語は次のとおり（2026-11-12、`ca6a0742` の `git ls-files` で、テストを除いた行数）。

- Rust: PreToolUse hook の `claude-hooks`、ADR の `design-records`、コミットの番号の `commit-refs`
- sh: `.claude/scripts/` と各スキルの `scripts/`、git hook（`git-hooks.nix` の中の sh と `staged-checks.sh`、
  `ja-terms/*.sh`、`scan-staged-secrets.sh`）、`claude-memory-backup.sh`、`claude-sync.sh`。合わせて約 5,400 行
- Python: `history_rewrite.py`、`prh_rules.py`、`usage-report.py`、`kept-sentences.py`、`fix-markdown-lint.py`。
  約 2,400 行
- テストの多くも sh（`.claude/tests/hooks.sh` は 3,197 行）

Windows の Claude Code は、command の hook を Git Bash で、Git Bash が無ければ PowerShell で実行する。`args` を書いた
exec form ではシェルを通さず、`command` は `.exe` などの実行ファイルでなければならない
（<https://code.claude.com/docs/en/hooks> の command hook の節、2026-11-12 に閲覧）。

Windows で sh のスクリプトが動かない理由は、sh そのものより次の依存にある。

- `jq`、`flock`（util-linux）、GNU coreutils と sed の挙動。Git for Windows には `jq` が入っていない
- Python。Windows には標準で入っていない
- Nix の store のパス。git hook と `claude-hooks` は、`ja-terms-message`、`commit-msg-check`、`scan-staged-secrets` を
  store のパスで呼ぶ
- git hook の中身も同じ依存を持つ。`staged-checks.sh` は `jq` と `python3`（`fix-markdown-lint.py`）を、
  `scan-staged-secrets.sh` は `jq` を使う。Git for Windows は git hook を同梱の sh で実行するので、sh そのものは動く

## 決定の要因

- hook がネイティブの Windows で、Linux と同じ判定をすること
- 実装の言語を減らし、同じ規則を言語ごとに書き分けないこと
- Linux での今の動き（Nix での配備、Slurm）を壊さないこと
- 依存を増やさないこと（ADR-0047 の数え方）

## 検討した案

- 案 A: sh と Python のまま、Windows では Git Bash と、別に入れた Python、`jq` に頼る
- 案 B: Windows 用に PowerShell で書き直し、Linux は sh のまま
- 案 C: Python に寄せる
- 案 D: hook と、開発の手元で使うスクリプトを Rust に寄せる。Slurm のスクリプトは sh のまま残す
- 案 E: Slurm のスクリプトも含め、すべてを Rust にする

## 決定と理由

採用: 案 D。

- 移す範囲: PreToolUse hook（すでに Rust）と、git hook、`.claude/scripts/` とスキルの `scripts/`、Python のツール、
  `claude-memory-backup`。テストは、移すときに Rust の結合テスト（`tests/`）に置き換える
- 残す範囲: Slurm のスクリプト（`slurm-run.sh`、`lib/slurm-job.sh`、`slurm-queue.sh`、`slurm-history.sh`、
  `lib/slurm-exclusive.sh`）と Nix のモジュール。Slurm は r995（Linux）でしか使わない
- 外のコマンド: `git`、`gh`、`gitleaks`、`squeue` は今のとおり子プロセスで呼ぶ。`jq` は serde_json、`flock` は
  ファイルのロック、coreutils・sed・awk・Python の処理は Rust の標準ライブラリか事実上の標準の crate に置き換える
- hook が呼ぶ外の検査（`ja-terms-message` などの store のパス）は、Windows ではパスが無いので、hook のバイナリが
  自分で持つか、PATH で見つける形に変える。どちらにするかは、移すときに検査ごとに決める。崩れた日本語の語の検査
  （ADR-0014、textlint と prh、Node.js）は、Node.js を Windows の依存として残すか、prh の辞書を Rust で読むかを
  ここで決める
- この ADR が決めないこと: Windows への配備（バイナリのビルドと置き場、settings.json への exec form の登録、
  `claude-sync` が担うスキルと CLAUDE.md の同期、git hook の設置）と、Windows での確かめ方。どちらも移す先の言語に
  よらず要る決定で、それぞれ別の ADR にする（#101 の段階 1 と 2）。`claude-sync.sh` を Rust に移すかも、配備の
  ADR で決める

理由:

- hook は Bash ツールの呼び出しごとに走るので、起動の速い 1 つの実行ファイルがよい。Rust のバイナリは exec form で
  シェルを通さずに起動でき、Git Bash の有無に左右されない
- 規則がすでに `claude-hooks` の Rust にあり、git hook と PreToolUse hook の判定を揃える（`block_main_commit.rs` の
  冒頭の注記）のも、同じ言語なら同じ関数を呼べる。git hook は sh のまま `jq` と Python を外す道もあるが、今は
  同じ判定を sh と Rust に書き分け、揃っているかを `modules/home/parts/tests/main-commit-gates.sh` で確かめている。
  Rust に移せば、この書き分けと照合が要らなくなる
- Rust の CLI の作り方（`package.nix` の `buildRustPackage`、`tests/` の結合テスト）は ADR-0012 で揃えてあり、
  Windows 向けには `cargo build` で同じソースからビルドできる
- 案 A は、Windows に `jq` と Python を別に入れる手順が要り、store のパスの問題も残る。sh の移植性（GNU と
  Git Bash の違い）を検査する手段も無い
- 案 B は、同じ規則を 2 つの言語で書き、揃っているかを確かめ続ける費用がかかる
- 案 C は、Windows に Python を入れる手順が要り、すでに Rust の hook とで 2 つの言語が残る
- 案 E は、Windows で使わない Slurm のスクリプトを移す費用に、得るものが無い。Slurm を Windows から使う要件が出たら
  見直す

### 帰結

- 良い点: hook とスクリプトが 1 つの言語になり、Windows と Linux で同じソースを使える。`jq` と Python が開発の手元の
  依存から外れる
- 悪い点: 崩れた日本語の語の検査を Rust に移さなければ、Node.js（textlint）が Windows の依存に残る
- 悪い点: ADR-0012 の帰結と同じく、振る舞いを変えるにはビルドが要り、その場で直して試せない。スキルのスクリプトは
  今 `claude-sync` がビルドせずにコピーしているので、配備の手順が変わる
- 悪い点: 移し終わるまで、sh と Rust の実装が並ぶ。移す順序と、Windows への配備の方式を決める段階は #101 に書く
- 実装はまだ決定に追いついていない。進みは #101 に書く

### 確認

- 移したスクリプトごとに、Linux の `nix flake check` と、Windows の `cargo test` が通る。Windows で `cargo test` を流す
  場所（手元か CI か）は、確かめ方の ADR で決める
- ネイティブの Windows で、PreToolUse hook が Linux と同じ入力に同じ判定を返す。判定は `claude-hooks` の Rust の
  テストが持つ入力と期待値で確かめるので、両方の OS で同じテストが通れば揃う。sh のテスト（`.claude/tests/hooks.sh`）
  の入力は、移すときに Rust のテストに移す
