---
status: accepted
date: 2026-11-11
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0080: nixfiles のセッションでは、ユーザーの CLAUDE.md を設定で除き、指示を 1 回だけ読む

## 背景

グローバルの指示（`~/.claude/CLAUDE.md`）の元は nixfiles の `.claude/CLAUDE.md` で、`claude-sync` が `~/.claude` に配る。
Claude Code は `./.claude/CLAUDE.md` をプロジェクトの指示として、`~/.claude/CLAUDE.md` をユーザーの指示として読む
（<https://code.claude.com/docs/en/memory> の「Choose where to put CLAUDE.md files」の表、2026-11-11 に閲覧）。
nixfiles とその worktree で始めたセッションは、同じ中身を毎回 2 回読んでいた（2026-11-11、Claude Code 2.1.322。main の
作業ツリーで `claude -p` に見出し「# グローバル開発指針」の回数を聞くと 2）。

## 決定の要因

- nixfiles のセッションが、指示を 1 回だけ読むこと
- ほかのリポジトリのセッションが読むもの（`~/.claude/CLAUDE.md`）が変わらないこと
- clone と worktree の置き場や名前によらず効くこと
- `~/.claude/CLAUDE.md` の無い環境（クラウドのセッションなど）で nixfiles を開いても、指示が読まれること

## 検討した案

- 案 A: 元のファイルの名前を、Claude Code が読まない名前（`.claude/GLOBAL.md`）に変える。同期先は `~/.claude/CLAUDE.md` のまま
- 案 B: ユーザーの設定（`~/.claude/settings.json`）の `claudeMdExcludes` で、nixfiles の `.claude/CLAUDE.md` を除く
- 案 C: プロジェクトの設定（nixfiles の `.claude/settings.json`）の `claudeMdExcludes` で、ユーザーの `~/.claude/CLAUDE.md` を除く
- 案 D: `.claude` のディレクトリ名を変える（`claude/` など）
- 案 E: 何もしない

## 決定と理由

採用: 案 C。`.claude/settings.json` に `"claudeMdExcludes": ["/home/*/.claude/CLAUDE.md"]` を書く。

- `claudeMdExcludes` は glob のパターンを絶対パスに当て、ユーザー、プロジェクト、ローカルのどの設定にも書ける（同じ文書の
  「Exclude specific CLAUDE.md files」）。プロジェクトの設定からユーザーの CLAUDE.md を除けることは、文書に明記が無いので
  試して確かめた（下の「確認」）
- 設定はリポジトリにあるので、どの clone と worktree でも効く。パターンの `*` は `/` をまたがないので、当たるのはホームの
  直下の `.claude/CLAUDE.md` だけで、ホームの下にある clone の `.claude/CLAUDE.md` には当たらない
- nixfiles のセッションは、作業中のブランチの `.claude/CLAUDE.md` を読む。指示を直すブランチでは、`claude-sync` を実行
  しなくても、その worktree のセッションに直した指示が効く
- 案 A は先に実装して捨てた（ユーザーの案。マージしていない）。
  `~/.claude/CLAUDE.md` の無い環境で nixfiles を開くと指示が 1 つも読まれず、worktree で直した指示は `claude-sync` を実行する
  まで効かない。`claude-sync`、そのテスト、ADR の番号の検査、採用中の ADR が書くパスにも変更が及んだ。案 C はどれも
  起こさない
- 案 B は、パターンがディレクトリの名前（`nixfiles`、`nixfiles-<ブランチから作った名前>`）に頼り、別の名前の clone では黙って
  2 回読みに戻る
- 案 D は、`.claude/` の下のパスを書いているファイル（確定した ADR の凍結する部分を含む）のすべてに変更が及び、指示の
  重複 1 つに見合わない。スキルは同じ名前のものが `~/.claude/skills` と `.claude/skills` にあるが、一覧に出るのは 1 回で、
  2 回は読まれていない（2026-11-11 のセッションで、/adr の読み込み元は `~/.claude/skills/adr`）

### 帰結

- 良い点: nixfiles のセッションが読む指示が 1 回になり、作業中のブランチの版になる
- 悪い点: main より古いブランチの worktree のセッションは、そのブランチの古い `.claude/CLAUDE.md` だけを読み、後から main に
  入った指示が効かない（前は、配られた `~/.claude/CLAUDE.md` の新しい版も読んでいた）。指示を使うセッションは、main に
  rebase した worktree で始める
- 悪い点: ホームが `/home/<ユーザー>` でない環境（macOS など）では、パターンが当たらず 2 回読みに戻る。今の対象のホストは
  どれも NixOS で `/home/<ユーザー>`。clone を `/home` の直下（`/home/nixfiles`）に置くと、プロジェクトの側にも当たり、指示が
  1 つも読まれない
- 悪い点: `.claude/settings.json` は、`.claude` の下でこれだけが `~/.claude` に配らないプロジェクトの設定になる。配ると、
  どのリポジトリでもグローバルの指示が読まれなくなる（`claude-sync` は配る対象を名前で挙げていて、このファイルは入っていない）
- 悪い点: nixfiles のセッションは、`~/.claude/CLAUDE.md` にだけある内容（同期の後に手で足したもの）を読まない。
  `~/.claude/CLAUDE.md` は生成物で、手では直さない運用

### 確認

- 2026-11-11、Claude Code 2.1.322: 使い捨てのプロジェクトに `.claude/CLAUDE.md` とこの設定を置いて `claude -p` に聞くと、
  プロジェクトの指示の見出しは有り、「グローバル開発指針」の見出しは無し。設定を外すと両方有り
- 同じ日、nixfiles の main の作業ツリーと、この設定を置いた worktree で、見出し「# グローバル開発指針」の回数を聞くと、
  2 と 1
- 自動のテストは無い（確かめるには Claude Code を起動して API を呼ぶ必要がある）。Claude Code の更新で指示が 2 回読まれて
  いると疑うときは、上の 2 つ目を繰り返す
