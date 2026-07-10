#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
expected="$repo_root/tests/expected_test_names.txt"
actual="$(mktemp)"
trap 'rm -f "$actual"' EXIT

cd "$repo_root"
cargo test --locked -- --list >"$actual" 2>&1

missing=0
while IFS= read -r test_name; do
    [[ -z "$test_name" ]] && continue
    if ! grep -Eq "(^|::)${test_name}: test$" "$actual"; then
        printf 'missing frozen test: %s\n' "$test_name" >&2
        missing=1
    fi
done <"$expected"

if [[ "$missing" -ne 0 ]]; then
    printf '\nFull cargo test inventory:\n' >&2
    cat "$actual" >&2
    exit 1
fi

printf 'verified %s frozen test names\n' "$(grep -cve '^$' "$expected")"
