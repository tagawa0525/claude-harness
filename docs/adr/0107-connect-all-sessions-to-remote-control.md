---
type: ADR
status: accepted
date: 2026-11-27
requires: [ADR-0084]
supersedes: []
superseded-by:
issues: ["#416"]
description: 静的な設定（claude-settings.json）に remoteControlAtStartup=true を置き、ハーネスを入れたすべての機械の対話セッションを Remote Control に自動で接続する
---

# ADR-0107: 静的な設定に remoteControlAtStartup=true を置き、すべての機械の対話セッションを Remote Control に自動で接続する

## 背景

ユーザーは、機械の全体を Remote Control の対象にすることを求めた（ユーザー 2026-11-25）。Claude Code 2.1.326 の公式の文書
（<https://code.claude.com/docs/en/remote-control>）には、機械を単位にする仕組みは無い。近いものは次の 2 つ:

- `remoteControlAtStartup`（`/config` の「Enable Remote Control for all sessions」）: 対話のプロセスが 1 つ起動するたびに、
  リモートのセッションを 1 つ登録する。手元で始めたセッションを claude.ai/code とモバイルのアプリから見て操作できる。
  文書が挙げる置き場は user の `~/.claude/settings.json` と managed settings の `true` で、プロジェクトの設定の `true` は
  無視される（`false` は効き、そのリポジトリだけ切る）。2.1.326 のバイナリは、どの設定のファイルにも無いときに
  旧来の `~/.claude.json` の `remoteControlAtStartup` を読む（バイナリの文字列で確かめた。2026-11-27）
- `claude remote-control`（server mode）: 起動したディレクトリで常駐し、リモートから新しいセッションを始められる。対象は
  そのディレクトリだけで、ホームディレクトリの信頼は保存されないので、端末の無い常駐の起点にできない

ユーザーはこの Windows の機械で `/config` から前者を有効にし、ほかの機械にも配ることを求めた（ユーザー 2026-11-27）。
`/config` は `~/.claude/settings.json` の最上位に `"remoteControlAtStartup": true` を書いた（2026-11-27 に確かめた）。
この機械の `~/.claude/settings.json` にしか書かないので、ほかの機械と新しく入れる機械には届かない。

静的な設定は、`claude-setup settings` が `~/.claude/settings.json` の最上位のキーごとに置き換え、ほかのキーは残す（ADR-0084、
`crates/claude-setup/src/settings.rs`）。

## 検討した案

- **静的な設定に `remoteControlAtStartup: true`**（採用）: ハーネスを入れたすべての機械に、配備のたびに入る
- **機械ごとに `/config` で有効にする**: 静的な設定に無いキーは配備で消えないので、機械ごとには残る。ただし新しく入れた
  機械では毎回手で有効にすることになり、どの機械が接続しているかをハーネスの設定から読めない
- **`claude remote-control` を機械ごとに常駐させる**: リモートから新しいセッションを始められるが、対象は起動した
  ディレクトリだけで、常駐と再起動（ネットワークが約 10 分切れるとプロセスが終わる）の仕組みは文書に無く、外側
  （systemd、タスクスケジューラ）に作ることになる。手元のセッションを外から見たいという今の求めには要らない。要るときは
  別の決定にする
- **プロジェクトの `.claude/settings.json` に置く**: `true` は無視されるので効かない

## 決定と理由

`claude-settings.json` に `"remoteControlAtStartup": true` を置く。ユーザーが求めたのはすべての機械であり、機械ごとの手作業を
残さず、接続の有無をハーネスの設定から読めるようにするため。

### 帰結

- ハーネスを入れたすべての機械で、対話のセッションが claude.ai のアカウントから操作できるようになる。その機械でセッションが
  できることは、アカウントにサインインした端末（ブラウザー、モバイル）からもできる。危険はアカウントと端末の保護に掛かる。
  権限の確認のダイアログは、リモートの端末に転送されて答えられる（既定で 5 分。`dialogExpiry`）。ただし確認を出さない
  モード（`--dangerously-skip-permissions` などの bypass。静的な設定は `skipDangerousModePermissionPrompt: true` で、
  その確認も省く）で起動したセッションでは、リモートの端末からの指示が確認なしで実行される
- 静的な設定の最上位のキーなので、機械で `/config` から切っても、次の配備で `true` に戻る。機械ごとに自動の接続だけを
  切る方法は無い。`disableRemoteControl` をその機械の `~/.claude/settings.json` に書くと、自動の接続に加えて
  `/remote-control`、`--remote-control`、`claude remote-control` もその機械で使えなくなる（2.1.326 のバイナリの設定の
  説明で確かめた。2026-11-27）。リポジトリごとに自動の接続だけを切るときは、そのプロジェクトの設定に
  `"remoteControlAtStartup": false` を書く
- Remote Control は、claude.ai のサブスクリプションでサインインした機械だけで使える（API キー、Bedrock、Vertex などでは
  使えない）。そうした機械でこのキーが `true` のときの挙動は、文書に書かれておらず確かめていない（#416）
- 1 つの対話のプロセスが登録するリモートのセッションは 1 つ。リモートから新しいセッションを始めることは、この決定では
  できない

### 確認

配備の後、各機械の `~/.claude/settings.json` に `"remoteControlAtStartup": true` があることと、新しく起動したセッションが
claude.ai/code のセッションの一覧に出ることを確かめる。この Windows の機械では、`/config` で有効にした後のセッションが
claude.ai/code の URL を持つことを確かめた（2026-11-27）。
