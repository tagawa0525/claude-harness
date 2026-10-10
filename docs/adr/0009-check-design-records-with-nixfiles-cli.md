---
status: accepted
date: 2026-10-11
requires: []
supersedes: [ADR-0008]
superseded-by:
issues: ["#269", "#270", "#280"]
---

# ADR-0009: 設計の記録の形は nixfiles に置いた CLI で作って検査し、各プロジェクトは pre-commit で PATH から呼ぶ

## 背景

ADR-0008 は、ADR の形をスキル `adr` と雛形の `new-adr.sh` で示し、検査は「検査の本体を 1 つの flake に置き、
各プロジェクトが git-hooks.nix で取り込む」とした。置き場所（nixfiles か、別のリポジトリか）は決めていなかった（#269）。

ADR の形を知っているのは、スキルと `new-adr.sh` だけではない。nixfiles のハーネスの `branch-topics.sh`
（ADR の status の変化）と、claude-hooks の `pre-merge-check`・`pre-git-merge-check` も、ADR の形を前提にしている。
ADR を持つ自分のプロジェクトは 4 つ（xlc 118 件 6f1d237、lsp-det 23 件 2d57a63、pleasanter-rs 7 件 16fb285、
nixfiles 8 件 927e800。2026-10-11 に各リポジトリの `docs/adr` で数えた）。GitHub Actions を使う自分のプロジェクトは、どれも言語の
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
- プロジェクトは pre-commit の設定（`.pre-commit-config.yaml`）に local hook を足してオプトインする。全体の hook では
  ないので、他人のプロジェクトかの判定が要らない
- 設定を走らせるのは prek（pre-commit 互換の実装）で、home-manager で PATH に入れる。グローバルの git hook
  （`core.hooksPath`）があると `prek install` は拒否されるので、グローバルの pre-commit hook が `prek run` を呼ぶ。
  オプトインしたプロジェクトでも、グローバルの hook の既定の検査（Nix、Python、Markdown、Rust）は続けて走らせる

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
- **運用**: proposed の間は自由に直し、実装をマージするときに accepted にする。確定した後に直すのは、誤字、文法、
  マークアップ、壊れたリンク、前付けの status、補足への日付つきの追記だけ（PEP 1、lsp-det の「追補」から）
- **ADR は増えていく前提にする**: LLM が決めてマージし、後で見直して作り直すことが多くなる。そのたびに新しい ADR で
  置き換え、前の判断の全文をファイルとして残す。今の決定は `status: accepted` で引く

ADR-0008 の決定のうち、次を変える:

- **雛形**: `new-adr.sh` の役目（全ブランチの履歴から番号を振り、前付けと見出しを書く）を CLI に移し、形の値
  （キー、status、必須と推奨の節）の持ち主を CLI だけにする。スキルと `branch-topics.sh` は CLI を呼び、Rust の
  claude-hooks は CLI のクレートをライブラリとして使う
- **運用**: deferred の間も proposed と同じく自由に直す。確定した後に直してよい前付けは、`status` に加えて
  `superseded-by` と `date`（置き換えたときに書き、status が変わった日にする）
- **検査**: ADR-0008 は「検査の本体を 1 つの flake に置き、各プロジェクトが git-hooks.nix で取り込む」としたが、
  取り込むには各プロジェクトが flake input で版を固定し、lock を更新し続けることになる（上の採らなかった案）ので
  退ける。CLI が検査し、各プロジェクトは pre-commit の local hook でオプトインする。オプトインにするのは、全体の
  hook（ADR-0006 の版）では、他人のプロジェクトかの判定に抜けが続いたため。元にするのは xlc の `crates/docs`
  （ADR と issue の前付け・参照・番号を検査する）。xlc に固有の部分（検査するファイルの範囲）は、pre-commit の設定
  （hook に渡すファイル）に出す。issue の書式（#270）が決まったら同じ CLI に足す。それまで xlc は、issue とソースの
  参照の検査に `crates/docs` を使い続け、ADR の検査だけを CLI に移す
- **置き換えの一致**: `supersedes` と `superseded-by` の双方向の一致は、置き換える側が効力を持ってから（accepted、
  superseded、deprecated）求める。proposed の ADR が `supersedes` に挙げた ADR は、まだ採用中なので
  `superseded-by` を書かない
- **CI**: 検査しない。ADR と issue は手元で書いてコミットするので、pre-commit で捕まる

### 帰結

- 良い点: 標準を変えると、rebuild で全プロジェクトが切り替わる。プロジェクトの lock は動かない
- 良い点: 形の値を持つのが CLI だけになり、スキル・ハーネスとずれない
- 悪い点: 版を固定しないので、古いコミットを当時の標準で検査し直せない。標準を厳しくすると、全プロジェクトが
  一斉に通らなくなる（一斉に直す前提）
- 悪い点: GitHub の画面での編集と `--no-verify` のコミットは素通りする。壊れた記録は、次に手元でコミットした人の
  検査で止まる
- 悪い点: 版はホストごとに、最後に rebuild したときのもの。rebuild していないホストは古い標準で検査する
- 悪い点: home-manager の設定を入れていない手元では、グローバルの hook も prek も無いので素通りする。prek が PATH に
  無いとき（PATH の狭い GUI のクライアントなど）は、グローバルの hook が知らせて既定の検査だけで通す。prek はあって
  CLI が無いときは、prek の hook が実行できずにコミットが止まる
- 悪い点: prek が PATH に入ったので、`.pre-commit-config.yaml` を持つ他人のプロジェクトでも、その設定がコミットの
  たびに走る（fork でも走らせる。設定はそのプロジェクトの規約）
- 実装が決定に追いついていない点: xlc、lsp-det、pleasanter-rs のオプトインは、rebuild の後に各リポジトリで行う
  （#280）。lsp-det と pleasanter-rs の既存の ADR は前付けが無く、xlc の ADR は `requires` が無い
- 実装が決定に追いついていない点: `branch-topics.sh` は、docs/adr と docs/issues のどのファイルを数えるかを、まだ
  自分の正規表現で決めている。issue の形を CLI に足すとき（#270）に、両方の判定を CLI に移す
- 見直す条件: 素通りが実際に起きたら CI での検査を足す。自分以外がコミットするプロジェクトができたら、版の固定を
  考え直す

### 確認

- CLI のテスト（`modules/home/parts/design-records`。雛形と番号、前付け、必須と推奨の節、番号の重複、置き換えの双方向
  の一致、参照先の有無、置き換えられた ADR への参照、確定した ADR の本文の変更、status の読み出し）
- グローバルの pre-commit hook が prek を呼び、既定の検査も走る（`modules/home/parts/tests/pre-commit-prek.sh`）
- nixfiles で `prek run` から検査が走る（`.pre-commit-config.yaml`）。xlc、lsp-det、pleasanter-rs は #280
- スキル `adr` と claude-hooks が CLI（またはそのクレート）を使い、自分で形を解釈していない。`branch-topics.sh` は
  status を CLI で読む

## 補足

- この決定までに、置き場所の推奨が 専用のリポジトリ → nixfiles の中の別の flake → 専用のリポジトリ → この案と
  変わった。変わったのは、ユーザーの意図（一斉に切り替えてよい、ハーネスの調整を任せたい）を後から知ったため

実装の途中で試して変えた版:

- **pre-commit で、すべての ADR の warning を出す版**: コミットのたびに、触っていない古い ADR の推奨の節の warning
  （nixfiles では 6 件）が並び、今のコミットの warning が埋もれる。hook に渡したファイルの warning だけを出し、error は
  ADR どうしの整合なのですべて出す
- **pre-commit の設定で `always_run` を付けた版**: テキストのファイルを含まないコミット（画像だけ、メッセージだけの
  amend）でも、ファイルを渡さずに検査が走り、すべての ADR の warning が並んだ。ADR どうしの違反は ADR を変える
  コミットでしか生まれないので外した
- **pre-commit の設定で `require_serial` を付けない版**: prek はファイルを分けて hook を並べて呼ぶので、
  `prek run --all-files` で ADR どうしの検査の指摘が呼んだ回数だけ重なった
