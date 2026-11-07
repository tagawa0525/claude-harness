#!/usr/bin/env python3
"""usage-report.py — Claude Code の会話の記録から、区間の費用（ドル）を親・担当・レビュー・振り返りに分けて出す

Usage: usage-report.py [--since <日時>] [--until <日時>] [--repo <リポジトリ>]... [TRANSCRIPT]
  TRANSCRIPT  セッションの記録（~/.claude/projects/<プロジェクト>/<セッション>.jsonl）。
              省略時は CLAUDE_CODE_SESSION_ID の記録を ~/.claude/projects/*/ から探す
  --since, --until
              応答の timestamp が区間 [since, until) に入るものだけを数える。日時は ISO 8601 で、時差
              （Z か +09:00 の形）を必ず付ける（シェルの TZ で意味が変わらないため）。両方を省くと JST の今日、
              片方だけなら、もう片方の端は開いたまま
  --repo      マージの数を数えるリポジトリ（複数可）。origin/HEAD があればそれ、無ければ HEAD の
              first-parent のマージコミットのうち、コミットの日時が区間に入るものを数える。origin/HEAD は
              fetch しないと古いので、呼ぶ前に fetch する

記録の隣の <セッション>/subagents/*.jsonl（サブエージェントの記録）も読む。サブエージェントの区分は
最初の依頼文で決める: `Review target:` で始まれば レビュー（/code-review が渡す依頼文）、最初に
`/<スキル> を` の形で呼ぶスキルが retro なら 振り返り、ほかは 担当。区間に応答の無い記録は数えない。

出力:
  TRANSCRIPT: <path>
  SINCE: <JST の日時>  UNTIL: <JST の日時>   区間の端（指定した端だけ）
  TOTAL: $<合計>
  COST: <区分> <本数> 本 $<費用> <割合>% 書き直し <回数> 回 $<その書き込みの費用>
  TOKENS: <区分> input=<数> write_5m=<数> write_1h=<数> read=<数> output=<数>
  REWRITES: <回数> 回 $<費用>   文脈の書き直し（1 回の応答で書き込みが 25 万トークンを超えたもの）の合計
  MERGES: <数> <リポジトリ> <ref>   --repo ごと
  PER_MERGE: $<費用>（マージ <数> 本） | -（--repo なし） | -（マージ 0 本）
  TOP: $<費用> <区分> <応答の数> 回 文脈の最大 <トークン> <始め>-<終わり> <依頼文の頭>   費用の大きい
                                     サブエージェント 10 本

1 回の応答は本文と tool_use などで複数の行に同じ message.id と usage で記録されるので、記録ごとに
message.id の 2 行目からを数えない。記録の形は Claude Code の公開の仕様ではないので、価格表に無いモデル、
書き込みの内訳（cache_creation）の和が合わない usage、標準でない速度・地域・tier は、推測せずに exit 1。
"""

import argparse
import datetime
import glob
import json
import os
import re
import subprocess
import sys

JST = datetime.timezone(datetime.timedelta(hours=9))

# ドル / 100 万トークン: (入力, 5 分の書き込み, 1 時間の書き込み, 読み出し, 出力)。
# 出典: https://platform.claude.com/docs/en/about-claude/pricing の「Model pricing」の表（2026-11-07 に閲覧。
# ページに版と更新日の表示は無い）。読み出しの倍率はモデルで違う（同じページの注: Fable 5.1 は入力の 0.025 倍、
# Opus 5.5 と Sonnet 5.5 は 0.05 倍、ほかは 0.1 倍）ので、倍率でなく表の値を持つ
PRICES = {
    "claude-fable-5-1": (10, 12.50, 20, 0.25, 50),
    "claude-opus-5-5": (4, 5, 8, 0.20, 20),
    "claude-sonnet-5-5": (2, 2.50, 4, 0.10, 10),
}
# Haiku 5.5 は 10 万トークンを超えるプロンプトで単価が上がる（同じ表の 2 行）。プロンプトの大きさは、
# 入力、書き込み、読み出しの和で見る（キャッシュした部分もプロンプトに入るため。ページに定義の記述は無い）
HAIKU_5_5 = ((0.10, 0.125, 0.20, 0.01, 0.50), (0.50, 0.625, 1, 0.05, 2.50))
HAIKU_5_5_LIMIT = 100_000

REWRITE_TOKENS = 250_000
KINDS = ("親", "担当", "レビュー", "振り返り")
TOKEN_KEYS = ("input", "write_5m", "write_1h", "read", "output")
ISO_WITH_OFFSET = re.compile(
    r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})$"
)


class ReportError(Exception):
    pass


def parse_bound(name, value):
    if not ISO_WITH_OFFSET.match(value):
        raise ReportError(
            f"{name} は時差つきの ISO 8601 で書いてください（例 2026-11-06T00:00+09:00、2026-11-05T15:00Z）: {value}"
        )
    return datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))


def find_transcript(path):
    if path:
        return path
    session = os.environ.get("CLAUDE_CODE_SESSION_ID")
    if not session:
        raise ReportError(
            "記録のパスを渡すか、CLAUDE_CODE_SESSION_ID を設定してください"
        )
    found = glob.glob(os.path.expanduser(f"~/.claude/projects/*/{session}.jsonl"))
    if len(found) != 1:
        raise ReportError(
            f"セッション {session} の記録が {len(found)} 件見つかりました（1 件のはず）"
        )
    return found[0]


def prices_for(model, prompt_tokens):
    if model == "claude-haiku-5-5":
        return HAIKU_5_5[prompt_tokens > HAIKU_5_5_LIMIT]
    if model not in PRICES:
        raise ReportError(
            f"価格表に無いモデル: {model}（PRICES に価格のページの値を足してください）"
        )
    return PRICES[model]


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            x.get("text", "")
            for x in content
            if isinstance(x, dict) and x.get("type") == "text"
        )
    return ""


def read_transcript(path, since, until):
    """(最初の依頼文, [(時刻, 費用, トークンの dict, 書き込み)]) を返す"""
    prompt = None
    seen = set()
    replies = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            try:
                d = json.loads(line)
            except json.JSONDecodeError:
                continue  # 走っている最中のセッションの書きかけの最後の行
            if not isinstance(d, dict):
                continue
            if d.get("type") == "user" and prompt is None:
                prompt = text_of(d.get("message", {}).get("content"))
            if d.get("type") != "assistant":
                continue
            m = d.get("message")
            u = m.get("usage") if isinstance(m, dict) else None
            if not isinstance(u, dict) or m.get("id") in seen:
                continue
            seen.add(m.get("id"))
            if not isinstance(d.get("timestamp"), str):
                raise ReportError(
                    f"timestamp の無い応答があります（記録の形が変わったかもしれません）: {path}"
                )
            t = datetime.datetime.fromisoformat(d["timestamp"].replace("Z", "+00:00"))
            if (since and t < since) or (until and t >= until):
                continue
            replies.append((t, *price_reply(path, m.get("model"), u)))
    return prompt or "", replies


def price_reply(path, model, u):
    cc = u.get("cache_creation") or {}
    tokens = {
        "input": u.get("input_tokens") or 0,
        "write_5m": cc.get("ephemeral_5m_input_tokens") or 0,
        "write_1h": cc.get("ephemeral_1h_input_tokens") or 0,
        "read": u.get("cache_read_input_tokens") or 0,
        "output": u.get("output_tokens") or 0,
    }
    if model == "<synthetic>" and not any(tokens.values()):
        return 0.0, tokens, (0, 0.0)  # API の失敗で Claude Code が書く応答
    if tokens["write_5m"] + tokens["write_1h"] != (
        u.get("cache_creation_input_tokens") or 0
    ):
        raise ReportError(
            f"書き込みの内訳（cache_creation）の和が cache_creation_input_tokens と合いません: {path}"
        )
    for key, standard in (
        ("speed", "standard"),
        ("inference_geo", "global"),
        ("service_tier", "standard"),
    ):
        if u.get(key) != standard:
            raise ReportError(
                f"usage.{key} が {u.get(key)!r} です（価格表は {standard} の値だけ）: {path}"
            )
    prompt_tokens = (
        tokens["input"] + tokens["write_5m"] + tokens["write_1h"] + tokens["read"]
    )
    price = prices_for(model, prompt_tokens)
    cost = sum(tokens[k] * p for k, p in zip(TOKEN_KEYS, price)) / 1e6
    write = tokens["write_5m"] + tokens["write_1h"]
    rewrite_cost = (tokens["write_5m"] * price[1] + tokens["write_1h"] * price[2]) / 1e6
    return cost, tokens, (write, rewrite_cost)


def kind_of(prompt):
    if prompt.startswith("Review target:"):
        return "レビュー"
    skill = re.search(r"/([\w:-]+) を", prompt)
    if skill and skill.group(1) == "retro":
        return "振り返り"
    return "担当"


def count_merges(repo, since, until):
    has_origin = (
        subprocess.run(
            ["git", "-C", repo, "rev-parse", "--verify", "-q", "origin/HEAD"],
            capture_output=True,
            check=False,
        ).returncode
        == 0
    )
    ref = "origin/HEAD" if has_origin else "HEAD"
    cmd = ["git", "-C", repo, "log", "--merges", "--first-parent", "--format=%cI", ref]
    r = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if r.returncode != 0:
        raise ReportError(f"{repo} のマージを数えられません: {r.stderr.strip()}")
    n = 0
    for line in r.stdout.split():
        t = datetime.datetime.fromisoformat(line)
        if (not since or t >= since) and (not until or t < until):
            n += 1
    return n, ref


def report(args):
    if args.since is None and args.until is None:
        today = datetime.datetime.now(JST).replace(
            hour=0, minute=0, second=0, microsecond=0
        )
        since, until = today, today + datetime.timedelta(days=1)
    else:
        since = parse_bound("--since", args.since) if args.since else None
        until = parse_bound("--until", args.until) if args.until else None
        if since and until and since >= until:
            raise ReportError(
                f"--since（{args.since}）は --until（{args.until}）より前にしてください"
            )
    transcript = find_transcript(args.transcript)
    if not os.access(transcript, os.R_OK):
        raise ReportError(f"記録を読めません: {transcript}")

    cost = dict.fromkeys(KINDS, 0.0)
    count = dict.fromkeys(KINDS, 0)
    tokens = {k: dict.fromkeys(TOKEN_KEYS, 0) for k in KINDS}
    rewrites = dict.fromkeys(KINDS, 0)
    rewrite_cost = dict.fromkeys(KINDS, 0.0)
    top = []

    def add(kind, replies):
        count[kind] += 1
        for _, c, tok, (write, wc) in replies:
            cost[kind] += c
            for k in TOKEN_KEYS:
                tokens[kind][k] += tok[k]
            if write > REWRITE_TOKENS:
                rewrites[kind] += 1
                rewrite_cost[kind] += wc

    _, replies = read_transcript(transcript, since, until)
    add("親", replies)
    subagents = sorted(
        glob.glob(
            os.path.join(transcript.removesuffix(".jsonl"), "subagents", "*.jsonl")
        )
    )
    for path in subagents:
        prompt, replies = read_transcript(path, since, until)
        if not replies:
            continue
        kind = kind_of(prompt)
        add(kind, replies)
        ctx = max(
            t["input"] + t["write_5m"] + t["write_1h"] + t["read"]
            for _, _, t, _ in replies
        )
        top.append(
            (
                sum(c for _, c, _, _ in replies),
                kind,
                len(replies),
                ctx,
                replies[0][0],
                replies[-1][0],
                prompt,
            )
        )

    total = sum(cost.values())
    print(f"TRANSCRIPT: {transcript}")
    if since:
        print(f"SINCE: {since.astimezone(JST).isoformat()}")
    if until:
        print(f"UNTIL: {until.astimezone(JST).isoformat()}")
    print(f"TOTAL: ${total:.2f}")
    for k in KINDS:
        share = cost[k] / total * 100 if total else 0.0
        print(
            f"COST: {k} {count[k]} 本 ${cost[k]:.2f} {share:.1f}% 書き直し {rewrites[k]} 回 ${rewrite_cost[k]:.2f}"
        )
    for k in KINDS:
        print(f"TOKENS: {k} " + " ".join(f"{t}={tokens[k][t]}" for t in TOKEN_KEYS))
    print(f"REWRITES: {sum(rewrites.values())} 回 ${sum(rewrite_cost.values()):.2f}")
    merges = 0
    for repo in args.repo:
        n, ref = count_merges(repo, since, until)
        merges += n
        print(f"MERGES: {n} {repo} {ref}")
    if not args.repo:
        print("PER_MERGE: -（--repo なし）")
    elif merges == 0:
        print("PER_MERGE: -（マージ 0 本）")
    else:
        print(f"PER_MERGE: ${total / merges:.2f}（マージ {merges} 本）")
    for c, kind, n, ctx, start, end, prompt in sorted(
        top, key=lambda r: r[0], reverse=True
    )[:10]:
        span = f"{start.astimezone(JST):%m-%d %H:%M}-{end.astimezone(JST):%H:%M}"
        head = " ".join(prompt.split())[:70]
        print(f"TOP: ${c:.2f} {kind} {n} 回 文脈の最大 {ctx:,} {span} {head}")


def main():
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument("--since")
    parser.add_argument("--until")
    parser.add_argument("--repo", action="append", default=[])
    parser.add_argument("transcript", nargs="?")
    try:
        report(parser.parse_args())
    except ReportError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
