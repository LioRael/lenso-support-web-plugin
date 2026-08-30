#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

test -f Cargo.toml
test -f src/lib.rs
test -f docs/plugin-card.md
test ! -e .gitkeep

if rg -n 'lenso-http-auth|lenso_http_auth|sqlx|axum' Cargo.toml src tests; then
  echo "Support Web must use direct Auth extraction and must not own transport or persistence." >&2
  exit 1
fi

rg -q 'lenso-capability-http-endpoint.*0\.2\.7' Cargo.toml
rg -q 'lenso-auth-sdk.*0\.2\.1' Cargo.toml
rg -q 'list_cases_with_context' src/lib.rs
rg -q 'transition_case_with_context' src/lib.rs
rg -q 'add_message_with_context' src/lib.rs
rg -q 'Console.*no.*UI-contribution|Console.*has no.*UI-contribution' README.md docs/plugin-card.md
