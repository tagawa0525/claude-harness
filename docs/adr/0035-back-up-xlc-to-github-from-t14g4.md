---
status: accepted
date: 2026-10-12
requires: [ADR-0018]
supersedes: []
superseded-by:
issues: []
---

# ADR-0035: xlc を t14g4 の systemd timer で r995 から GitHub にミラーする

## 背景

xlc の origin は r995 の作業用リポジトリ（`r995:/home/tagawa/github/xlc`）で、GitHub のリモートを持たない。xlc の
手順（/git-merge など）は origin が r995 であることを前提にしている。r995 のディスクが壊れると、ブランチと、記録の番号を
予約するタグ（ADR-0018）が失われる。

2026-10-12 に公開リポジトリ github.com/tagawa0525/xlc を作り、t14g4 から r995 の全ブランチとタグを手で push した。
これを定期的に続ける。

## 検討した案

- 案 A（採用）: t14g4 の systemd user timer が、専用の bare リポジトリに r995 の `refs/heads/*` と `refs/tags/*` を
  fetch し、`--prune` と force の refspec で GitHub に push する
- 案 B: 作業用の clone（`~/github/xlc`）に remote `github` を足し、`refs/remotes/origin/*` を GitHub の
  `refs/heads/*` に push する。`refs/remotes/origin/HEAD`（symref）が `HEAD` というブランチとして送られる。
  negative refspec `^refs/remotes/origin/HEAD` を足しても外れなかった（git 2.55.0、2026-10-12 の `--dry-run`）。
  ブランチを列挙して渡すと `--prune` が効かない（消すブランチは wildcard の refspec からしか決まらない）
- 案 C: `git clone --mirror` と `git push --mirror`。`refs/*` をすべて送るので、r995 の作業用リポジトリに
  `refs/stash` などができると公開リポジトリに出る
- 案 D: force しない（fast-forward だけ送る）。r995 で rebase したブランチ（2026-10-12 の fetch でも
  `feat/p3-xlc` が forced update）が GitHub で古いまま残る
- 案 E: r995 で timer を実行する。origin を直接読めるが、バックアップは t14g4 を通す（ユーザーの指示）

## 決定と理由

案 A を採る。バックアップは r995 の今のブランチとタグのミラーにし、rebase と削除も反映する。作業用の clone の remote と
`refs/remotes` に触れないので、xlc の手順に影響しない。送る ref を heads と tags に限り、作業用リポジトリのほかの ref を
公開しない。

- スクリプト `xlc-github-backup` は `~/.local/state/xlc-github-backup/xlc.git` を中継に使い、無ければ `git init --bare` で作る
- 1 時間ごと（`OnCalendar=hourly`）。t14g4 はノート PC なので、`Persistent=true` で止まっていた間の分を起動後に実行する
- Claude Code の hook は `+` の refspec の push を Claude のコマンドとしては止める。この timer はユーザーの自動化として
  動き、hook を通らない。ミラーの先は作業の origin ではなく、force で上書きするものは r995 に残っている

### 帰結

- GitHub の xlc は、最大 1 時間遅れの r995 のミラーになる。GitHub だけにあるブランチやタグは、次の実行で消える
- r995 か t14g4 がオフラインの間は失敗し、journal に残る。次の実行で追いつく
- 作業用の clone に手で足した remote `github` は、この timer では使わない

### 確認

- `systemctl --user start xlc-github-backup.service` が成功し、`git ls-remote` の heads と tags が r995 と GitHub で一致する
- `systemctl --user list-timers xlc-github-backup.timer` に次の実行が出る
