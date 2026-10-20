#!/usr/bin/env bash
# run-checks.sh の Python のテストの段が呼ぶ（nixfiles の ADR-0036）。nixfiles の Python のテストは、
# パッケージの installCheckPhase が依存（git-filter-repo、pyyaml、regex、textlint、commit-refs）と一緒に実行するので、
# パッケージをビルドする（PATH の python3 には依存が無い）
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
# ここで実行するのは history-rewrite のテストだけなので、ほかの場所に足したテストを黙って飛ばさない
others=$(git ls-files -- 'test_*.py' '*/test_*.py' '*_test.py' | grep -v '^modules/home/parts/history-rewrite/' || true)
if [ -n "$others" ]; then
  echo "ERROR: scripts/run-python-tests.sh が実行しない Python のテストがある（ここに足す）:" >&2
  echo "$others" >&2
  exit 1
fi
nix build --no-link --print-build-logs .#checks.x86_64-linux.history-rewrite
