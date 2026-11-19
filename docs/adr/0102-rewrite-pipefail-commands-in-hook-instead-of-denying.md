---
type: ADR
status: proposed
date: 2026-11-20
requires: [ADR-0042, ADR-0084]
supersedes: []
superseded-by:
issues: ["#355", "#37", "#387"]
description: require-pipefail は止める形を拒否せず、許可の判断を返さない updatedInput で command の前に set -o pipefail; を付けて通し、付けたことを additionalContext で知らせる。ほかのルールが拒否する呼び出しでは拒否だけを返す
---

# ADR-0102: require-pipefail は、止める形を拒否せず、判断を返さない updatedInput で set -o pipefail; を前に付けて通し、additionalContext で知らせる

## 背景

require-pipefail（`crates/claude-hooks/src/rules/require_pipefail.rs`）は、`nix build`、`nix flake check`、`cargo`、
`run-checks.sh` を、後ろのステージに tail か head があるパイプの途中に置き、pipefail のシェルのオプションが無い形を
拒否していた。エスケープは無く、拒否の後のモデルの動きは、`set -o pipefail;` を前に付けて同じコマンドを打ち直す
ことにほぼ決まっている。xlc-e5 のセッションでは 2026-11-19 の約 3 時間に、親と担当の合計で 17 回の拒否と打ち直しが
あった（#355 の本文。xlc-e5 の振り返りが伝えた数で、このリポジトリでは数えていない）。

hook の文書（https://code.claude.com/docs/en/hooks の PreToolUse decision control、2026-11-20 に読んだ）は、
`hookSpecificOutput.updatedInput` がツールの入力の全体を置き換え、許可の規則は hook が返した入力に対して評価され、
`additionalContext` はツールの結果と一緒にモデルの文脈に入る、としている。`permissionDecision` を返さない
`updatedInput` は、auto mode では「a hook changed this call's input after auto mode reviewed it」で拒否されうる、
とだけ書き、ほかのモードでの振る舞いは書いていない。

段 1 の確認として #355 で（2026-11-20、Claude Code 2.1.327、`claude -p --permission-mode default`。
<https://github.com/tagawa0525/claude-harness/issues/355#issuecomment-7022034363>）、判断を返さずに
`updatedInput` だけを返す hook を動かして確かめた:

- 書き換えた後のコマンドが実行された（`false | tail -1` が `set -o pipefail; false | tail -1` になり、終了コード 1）
- 許可の規則は書き換えた後のコマンドに対して評価され、許可の無い元のコマンドの部分（`touch`）は確認のまま残った
- `additionalContext` はモデルに届いた
- `set -o pipefail` の部分は、それ自体に許可の規則（`Bash(set -o pipefail:*)`）が無いと確認になった。モデルが自分で
  `set -o pipefail;` を付けて打ったときも同じだった

## 決定の要因

- 元のコマンドの許可の確認と、ほかのルールの判定を弱めない（やり直しの 1 往復を無くすことより優先する）
- 書き換えたことをモデルが知る（知らないと、終了コードが変わった理由が分からない）
- 判定（どの形を対象にするか）は変えない。誤って対象にする形を直すのは #37

## 検討した案

1. 今の拒否のまま。モデルが `set -o pipefail;` を付けて打ち直す
2. `permissionDecision` を返さず、`updatedInput` で `command` の前に `set -o pipefail;` と空白 1 つを付け、`additionalContext` で知らせる
3. 2 に `permissionDecision: "allow"` を足す
4. 2 に `permissionDecision: "ask"` を足し、書き換えた入力をユーザーに見せる

## 決定と理由

案 2 を採る。require-pipefail が対象にする形で、ほかのルールが拒否しないときは、拒否の代わりに、入力のほかの欄を
そのままにして `command` の前に `set -o pipefail;` と空白 1 つを付けた `updatedInput` と、付けたことを知らせる
`additionalContext` を返す。`permissionDecision` は返さない。ほかのルールが同じ呼び出しを拒否するときは、拒否だけを
返し、書き換えは返さない（書き換えの文も入れない）。

- 案 1 は、拒否の文を読んで同じ前置きを付けるだけの 1 往復が、毎回残る
- 案 3 は、`allow` が許可の確認を飛ばすので、`set -o pipefail;` を付けるだけの書き換えで、元のコマンドの確認まで
  飛ぶ（文書の `permissionDecision` の `"allow"` skips the permission prompt）
- 案 4 は、許可の規則で確認なしに通るはずのコマンドにも、毎回ユーザーの確認を挟む。今の打ち直しより手間が増える
- 案 2 は、段 1 のとおり許可の規則が書き換えた後のコマンドに対して評価されるので、元のコマンドの確認は残る。
  付け足すのは pipefail のシェルのオプションだけで、走るコマンドは変えない
- 拒否を優先するのは、拒否された呼び出しは走らず、書き換えに意味が無いため。書き換えの文を拒否と一緒に返すと、
  走らなかったコマンドに「付けた」と知らせることになる。モデルが拒否の後に直したコマンドは、require-pipefail が
  もう一度見て書き換える

### 帰結

- ルールの結果に、入力を書き換える種類（`Finding::Prepend`。`command` の前に付ける文字列と、知らせる文）を足す。
  hook が入力を書き換えるのはこのハーネスで初めてで、ほかのルールが同じ形で書き換えるときもこの種類を使う
- 書き換えは拒否でないので、拒否の記録（`deny_log`）と、振り返りが数える `[<ルール名>]` の拒否には入らない
- 判定が誤って対象にする形（`cargo metadata | head`、`cargo test --help | head`。#37）は、拒否の代わりに pipefail が
  付く。head が先に閉じると、前のコマンドが SIGPIPE で止まり、終了コードが 141 になりうる。今のモデルが拒否の後に
  `set -o pipefail;` を付けて打ち直した結果と同じなので、この決定では扱わず、知らせる文に 1 文入れる
- `set -o pipefail` の部分は、許可の規則に無いと確認になる（段 1）。打ち直しでも同じだったので、書き換えで増える
  確認ではない。`claude-settings.json` に許可を足すかは #387
- auto mode では、文書のとおり、書き換えた呼び出しが拒否されうる（試していない）
- 複数の hook が同じ呼び出しに `updatedInput` を返したときの扱いは、文書に無い。PreToolUse の hook は claude-hooks の
  1 件だけ（ADR-0084）なので、今は当たらない。claude-hooks の中では、書き換えの前置きを評価の順につなぐ

### 確認

- `claude/tests/hooks.sh` の require-pipefail のテスト: 対象の形で拒否でなく、`set -o pipefail;` と空白 1 つを付けた
  `updatedInput`（`run_in_background` はそのまま）と `additionalContext` を返し、`permissionDecision` を返さない。
  pipefail がある形と対象でない形は何も返さない。ほかのルール（block-no-verify）が拒否する呼び出しでは、拒否だけを返す
- `crates/claude-hooks/src/output.rs` の単体テスト: 書き換えと拒否、書き換えとほかの知らせの組み合わせの出力
- 配備の後の実際のセッションで、`cargo --version | tail -1` が打ち直しなしで通ること（PR の報告に手順を書く）
