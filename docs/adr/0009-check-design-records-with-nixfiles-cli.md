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
ADR を持つ自分のプロジェクトは 4 つ（xlc 118 件 6f1d2371、lsp-det 23 件 2d57a63、pleasanter-rs 7 件 16fb285、
nixfiles。2026-10-11 に各リポジトリの `docs/adr` で数えた）。GitHub Actions を使う自分のプロジェクトは、どれも言語の
道具で CI を回し、Nix を使っていない（2026-10-11 に ~/github の各 `.github/workflows` で確かめた）。

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
- nixfiles を flake input にする: nixfiles の lock の 29 ノード（flake.lock、927e800、2026-10-11）と毎日の lock 更新が、
  各プロジェクトに入る

ADR-0008 の決定のうち、次はそのまま引き継ぐ:

- **形の基準**: MADR 4.0.0。スキル `adr` には MADR へのリンクと違う点だけを書き、それだけを読んで ADR を書けるように
  する。見出しは日本語（英語の原文でもよい）。節を足すことは許し、必須は背景・検討した案・決定と理由、推奨は帰結・確認
- **前付け**: 置き換えと参照を grep で引けるよう、`requires` / `supersedes` / `superseded-by` / `issues` を独立した
  キーにする（xlc の前付けと PEP の Requires から。MADR は置き換えを status の文中に書く）。status に `withdrawn` と
  `deferred` を足す（PEP、KEP）
- **ADR にするもの**: 作り直したときに他へ波及する決定と、範囲、期間、危険、費用のどれかが大きい決定
- **運用**: proposed と deferred の間は自由に直し、実装をマージするときに accepted にする。確定した後に直すのは、誤字、
  文法、マークアップ、壊れたリンク、前付けの status、補足への日付つきの追記だけ（PEP 1、lsp-det の「追補」から）
- **ADR は増えていく前提にする**: LLM が決めてマージし、後で見直して作り直すことが多くなる。そのたびに新しい ADR で
  置き換え、前の判断の全文をファイルとして残す。今の決定は `status: accepted` で引く

ADR-0008 の決定のうち、次を変える:

- **雛形**: `new-adr.sh` の役目（全ブランチの履歴から番号を振り、前付けと見出しを書く）を CLI に移し、形の値
  （キー、status、必須と推奨の節）の持ち主を CLI だけにする。スキルとハーネスは CLI を呼ぶ
- **検査**: CLI が行う。元にするのは xlc の `crates/docs`（ADR と issue の前付け・参照・番号を検査する）。
  xlc に固有の部分（検査するファイルの範囲）は設定に出す。issue の書式（#270）が決まったら同じ CLI に足す。それまで
  xlc は、issue とソースの参照の検査に `crates/docs` を使い続け、ADR の検査だけを CLI に移す
- **CI**: 検査しない。ADR と issue は手元で書いてコミットするので、pre-commit で捕まる

### 帰結

- 良い点: 標準を変えると、rebuild で全プロジェクトが切り替わる。プロジェクトの lock は動かない
- 良い点: 形の値を持つのが CLI だけになり、スキル・ハーネスとずれない
- 悪い点: 版を固定しないので、古いコミットを当時の標準で検査し直せない。標準を厳しくすると、全プロジェクトが
  一斉に通らなくなる（一斉に直す前提）
- 悪い点: GitHub の画面での編集と `--no-verify` のコミットは素通りする。壊れた記録は、次に手元でコミットした人の
  検査で止まる
- 悪い点: 版はホストごとに、最後に rebuild したときのもの。rebuild していないホストは古い標準で検査する
- 悪い点: CLI が PATH に無い環境（nixfiles の外の手元、PATH の狭い GUI のクライアント）では、hook が実行できずに
  コミットが止まる
- 見直す条件: 素通りが実際に起きたら CI での検査を足す。自分以外がコミットするプロジェクトができたら、版の固定を
  考え直す

### 確認

- CLI のテスト（雛形、前付け、必須の節、番号の重複、置き換えの双方向の一致、参照先の有無）
- nixfiles、xlc、lsp-det、pleasanter-rs で pre-commit から検査が走る
- スキル `adr`、`branch-topics.sh`、claude-hooks が CLI を呼び、自分で形を解釈していない

## 補足

- この ADR が accepted になるとき（#269 の実装をマージするブランチ）、ADR-0008 に `superseded-by: ADR-0009` と
  `status: superseded` を書く。それまでは ADR-0008 が採用中の決定
- この決定までに、置き場所の推奨が 専用のリポジトリ → nixfiles の中の別の flake → 専用のリポジトリ → この案と
  変わった。変わったのは、ユーザーの意図（一斉に切り替えてよい、ハーネスの調整を任せたい）を後から知ったため
