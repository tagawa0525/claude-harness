---
status: proposed
date: 2026-10-11
requires: []
supersedes: [ADR-0008]
superseded-by:
issues: ["#269", "#270"]
---

# ADR-0009: 設計の記録の形は nixfiles に置いた CLI で作って検査し、各プロジェクトは pre-commit で PATH から呼ぶ

## 背景

ADR-0008 は、ADR の形をスキル `adr` と雛形の `new-adr.sh` で示し、検査は「検査の本体を 1 つの flake に置き、
各プロジェクトが git-hooks.nix で取り込む」とした。置き場所（nixfiles か、別のリポジトリか）は決めていなかった（#269）。

ADR の形を知っているのは、スキルと `new-adr.sh` だけではない。nixfiles のハーネスの `branch-topics.sh`
（ADR の status の変化）と、claude-hooks の `pre-merge-check`・`pre-git-merge-check` も、ADR の形を前提にしている。
ADR を持つ自分のプロジェクトは 4 つ（xlc 118 件、lsp-det 23 件、pleasanter-rs 7 件、nixfiles）。GitHub Actions を
使う自分のプロジェクトは、どれも言語の道具で CI を回し、Nix を使っていない（2026-10-11 に ~/github で確かめた）。

## 決定の要因

- 自分がオーナーのプロジェクトは、標準を変えたら全部一斉に切り替えてよい（ユーザーの意図）
- 形を知るものを 1 か所にまとめ、スキル、雛形、検査、ハーネスがずれない
- ハーネスの調整を LLM に任せやすい（1 つの PR で直してテストし、すぐ効く）
- 枠は一般的な道具を使い、自作は中身だけにする

## 検討した案

- nixfiles に CLI を置き、各プロジェクトは pre-commit の local hook で PATH から呼ぶ
- 専用のリポジトリに置き、各プロジェクトが flake input で版を固定して取り込む
- nixfiles の中の別の flake（`?dir=`）に置き、各プロジェクトが flake input で取り込む
- nixfiles の flake の出力にし、各プロジェクトが nixfiles を flake input にする

## 決定と理由

採用: 「nixfiles に CLI を置き、各プロジェクトは pre-commit の local hook で PATH から呼ぶ」。

- 一斉に切り替えてよいので、版を固定する利点が無い。nixfiles を rebuild すれば全プロジェクトが新しい標準になる
- スキル、雛形、検査、ハーネスが 1 つのリポジトリにあり、1 つの PR で一緒に直してテストできる
- プロジェクトは pre-commit（prek）の設定に hook を足してオプトインする。全体の hook ではないので、他人の
  プロジェクトかの判定が要らない

採らなかった案:

- 専用のリポジトリで版を固定: 一斉に切り替えるたびに、全プロジェクトで lock を更新する。変更が効くのは nixfiles の
  lock を update で更新した後で、ハーネスとは 2 つのリポジトリにまたがる
- nixfiles の中の別の flake: 同じく lock の更新が要り、nixfiles の無関係なコミットでも rev が動く
- nixfiles を flake input にする: nixfiles の 29 ノードと毎日の lock 更新が、各プロジェクトに入る

ADR-0008 の決定は、次を変えて引き継ぐ:

- **形**: MADR 4.0.0 を基準にし、スキル `adr` には MADR へのリンクと違う点（日本語の見出し、前付けの
  `requires` / `supersedes` / `superseded-by` / `issues`、status の `withdrawn` と `deferred`、ADR にする基準、
  確定後の運用）だけを書く。ADR は置き換えで増えていく前提にする
- **雛形**: `new-adr.sh` の役目（全ブランチの履歴から番号を振り、前付けと見出しを書く）を CLI に移し、形の値
  （キー、status、必須と推奨の節）の持ち主を CLI だけにする。スキルとハーネスは CLI を呼ぶ
- **検査**: CLI が行う。元にするのは xlc の `crates/docs`（ADR と issue の前付け・参照・番号を検査する）。
  xlc に固有の部分（検査するファイルの範囲）は設定に出す。issue の書式（#270）が決まったら同じ CLI に足す
- **CI**: 検査しない。ADR と issue は手元で書いてコミットするので、pre-commit で捕まる

### 帰結

- 良い点: 標準を変えると、rebuild で全プロジェクトが切り替わる。プロジェクトの lock は動かない
- 良い点: 形の値を持つのが CLI だけになり、スキル・ハーネスとずれない
- 悪い点: 版を固定しないので、古いコミットを当時の標準で検査し直せない。標準を厳しくすると、全プロジェクトが
  一斉に通らなくなる（一斉に直す前提）
- 悪い点: GitHub の画面での編集と `--no-verify` のコミットは素通りする
- 悪い点: nixfiles の環境が無いところ（他人の手元、CI）では検査できない
- 見直す条件: 素通りが実際に起きたら CI での検査を足す。自分以外がコミットするプロジェクトができたら、版の固定を
  考え直す

### 確認

- CLI のテスト（雛形、前付け、必須の節、番号の重複、置き換えの双方向の一致、参照先の有無）
- nixfiles、xlc、lsp-det、pleasanter-rs で pre-commit から検査が走る
- スキル `adr`、`branch-topics.sh`、claude-hooks が CLI を呼び、自分で形を解釈していない

## 補足

- この決定までに、置き場所の推奨が 専用のリポジトリ → nixfiles の中の別の flake → 専用のリポジトリ → この案と
  変わった。変わったのは、ユーザーの意図（一斉に切り替えてよい、ハーネスの調整を任せたい）を後から知ったため
