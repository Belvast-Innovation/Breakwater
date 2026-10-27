#!/usr/bin/env bash
# SessionStart hook for Claude Code cloud sessions (.claude/settings.json).
#
# Local sessions exit immediately. In a cloud session, pre-fetch the locked
# dependency graph so the first build does not stall on the network. The hook
# never fails the session: every outcome exits 0.

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

export PATH="$HOME/.cargo/bin:$PATH"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || exit 0
cd "$repo_root" || exit 0

cargo fetch --locked >/dev/null 2>&1 || true

exit 0
