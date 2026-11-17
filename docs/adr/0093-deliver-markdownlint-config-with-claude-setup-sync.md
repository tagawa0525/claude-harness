---
type: ADR
status: proposed
date: 2026-11-17
requires: [ADR-0084]
supersedes: []
superseded-by:
issues: ["#265"]
---

# ADR-0093: markdownlint のグローバル設定は、.claude の 1 ファイルを claude-setup sync が ~/.config/markdownlint/config に配る

## 背景

ハーネスの検査（run-checks、staged-checks、fix-markdown-lint）は markdownlint を PATH から呼び、設定を持たないリポジトリでは
グローバルの設定（MD013、MD033 などを無効にする規則）が効く前提で動く。この設定は `git-hooks.nix` が
`home.file.".markdownlintrc"` で配っていた（Nix の属性セットを `builtins.toJSON` で書く）。Nix のホストにしか届かず、
claude-setup で配備する Windows のホスト（ADR-0084）では、設定を持たないリポジトリで markdownlint の既定の規則で検査される
（#265）。

markdownlint-cli 0.50.0 は、カレントディレクトリに設定が無いとき、ホームの下の次の場所を読む（2026-11-17、r995 で、HOME を
一時のディレクトリにして、MD013 の指摘の有無で確かめた）。

| 置き場所                             | 読まれるか |
| ------------------------------------ | ---------- |
| `~/.markdownlintrc`                  | 読まれる   |
| `~/.config/markdownlint`             | 読まれる   |
| `~/.config/markdownlint/config`      | 読まれる   |
| `~/.config/markdownlint/config.json` | 読まれない |
| `~/.markdownlint/config`             | 読まれる   |

## 決定の要因

- 設定の正を 1 つにする（Nix と Windows で規則がずれない）
- ハーネスの配備の仕組みに乗せる（Windows には Nix が無く、claude-setup sync だけが `.claude/` を配る）
- ホーム直下を増やさない

## 検討した案

### 案 A: `.claude/markdownlint.json` を 1 つの正とし、claude-setup sync が `~/.config/markdownlint/config` に配る（採用）

sync は `~/.claude` の下だけを扱っていた。ホームの下の行き先を持つファイルの一覧を足し、ソースが正として上書きする。
home-manager の activation も `claude-setup sync` を呼ぶので、Linux も同じ仕組みで配る（Nix の側では配らない）。

- 長所: 正が 1 ファイル。Nix と Windows の仕組みが 1 つ。XDG の置き場所
- 短所: Linux で、宣言的な home-manager のファイル（store への symlink）から、activation が書く通常のファイルに変わる

### 案 B: `~/.claude/markdownlint.json` に配り、ハーネスの検査が `--config` で渡す

- 長所: `~/.claude` の下だけで済み、sync を変えない
- 短所: 手で実行する `markdownlint` は読まない。ハーネスの検査の呼び出し（`staged_checks.rs`、`run_checks.rs`）が増える

### 案 C: 何もせず、Windows の手順（docs/windows.md）に設定の置き方を書く

- 長所: 実装が無い
- 短所: 設定が 2 か所（Nix の属性セットと手順の JSON）になり、ずれる

### 案 D: Nix の `xdg.configFile` で配り、sync は Windows だけで配る

- 長所: Linux は宣言的なまま
- 短所: 正が Nix と `.claude/` の 2 つになる（`fromJSON` で 1 つに寄せても、sync が Linux で symlink の先の store を書こうとして
  衝突するので、sync が symlink を飛ばす規則が要る）

## 決定と理由

案 A。`.claude/markdownlint.json` を正とし、`claude-setup sync` が `~/.config/markdownlint/config`（ホームからの相対）に置く。

- 中身が違えば上書きする（`CLAUDE.md` と同じく、ソースが正）。行き先が symlink なら、リンクを辿らず通常のファイルに置き換える
- ソースに無ければ何もしない
- `git-hooks.nix` の `home.file.".markdownlintrc"` は消す。`markdownlint-cli` の本体は Nix が入れ続ける
- 引数なしの同期が clone の未コミットの変更を見る場所（`pull` の検査）に、このソースも含める

### 帰結

- Linux の設定は、home-manager の宣言でなく、activation（と手動の `claude-sync`）が置く通常のファイルになる。利用者が手で
  直した設定は、次の同期で上書きされる
- 古い `~/.markdownlintrc`（home-manager の symlink）は、`git-hooks.nix` から消えた世代の switch で home-manager が外す
- Windows で `~` が `%USERPROFILE%` に解かれ、`%USERPROFILE%\.config\markdownlint\config` が markdownlint に読まれるかは、
  実機で確かめていない（上の表は Linux）。確かめて、結果をこの節に書く
- VSCode の markdownlint 拡張がこの場所を読むかは、前の `~/.markdownlintrc` のときから確かめていない

### 確認

- `modules/home/parts/claude-setup/tests/sync.rs` の `delivers_the_markdownlint_config_outside_claude` ほか: 配る、上書きする、
  ソースが無ければ置かない、symlink を置き換える、未コミットの変更で止める
- Windows で markdownlint が置いた設定を読むかは、このテストでは確かめない（markdownlint を入れていない）
