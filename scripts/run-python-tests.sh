#!/usr/bin/env bash
# run-checks.sh の Python のテストの段が呼ぶ（nixfiles の ADR-0036）。nixfiles の Python のテストは、
# パッケージの installCheckPhase が依存（git-filter-repo、pyyaml、regex、textlint、commit-refs）と一緒に実行するので、
# パッケージをビルドする（PATH の python3 には依存が無い）
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
nix build --no-link --print-build-logs .#checks.x86_64-linux.history-rewrite
