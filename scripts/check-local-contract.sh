#!/usr/bin/env bash
set -euo pipefail

plan="gg/zodex-local/zodex-local-implementation-plan-2026-08-15.md"
acceptance="gg/zodex-local/zodex-local-acceptance.md"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

# The acceptance closure deliberately derives the criterion set from the
# current plan instead of baking in the planning-time count. Any later
# Amendment that adds/removes/renumbers a criterion must force an explicit
# review of the evidence map.
awk -v criteria_lines="$tmpdir/plan-criteria" '
  /^## Acceptance Criteria$/ { in_criteria = 1; next }
  /^## Plan Phases$/ { in_criteria = 0 }
  in_criteria && /^[0-9]+[.] / {
    criterion = $1
    sub(/[.]$/, "", criterion)
    print criterion
    print $0 > criteria_lines
  }
' "$plan" > "$tmpdir/plan-ids"

plan_fingerprint="$(cksum "$tmpdir/plan-criteria" | awk '{ print $1 " " $2 }')"
reviewed_fingerprint="$(sed -n 's/^<!-- plan-acceptance-cksum: \([0-9][0-9]* [0-9][0-9]*\) -->$/\1/p' "$acceptance")"
if [[ -z "$reviewed_fingerprint" || "$plan_fingerprint" != "$reviewed_fingerprint" ]]; then
  echo "Zodex Local acceptance criteria changed since the evidence map was reviewed" >&2
  echo "current plan fingerprint: $plan_fingerprint" >&2
  echo "reviewed map fingerprint: ${reviewed_fingerprint:-missing}" >&2
  exit 1
fi

plan_file_fingerprint="$(cksum "$plan" | awk '{ print $1 " " $2 }')"
reviewed_plan_file_fingerprint="$(sed -n 's/^<!-- plan-file-cksum: \([0-9][0-9]* [0-9][0-9]*\) -->$/\1/p' "$acceptance")"
if [[ -z "$reviewed_plan_file_fingerprint" || "$plan_file_fingerprint" != "$reviewed_plan_file_fingerprint" ]]; then
  echo "Zodex Local implementation plan changed since the Phase 12 evidence map was reviewed" >&2
  echo "current plan-file fingerprint: $plan_file_fingerprint" >&2
  echo "reviewed plan-file fingerprint: ${reviewed_plan_file_fingerprint:-missing}" >&2
  exit 1
fi

awk -F'|' '
  /<!-- acceptance-map:start -->/ { in_map = 1; next }
  /<!-- acceptance-map:end -->/ { in_map = 0 }
  in_map && $2 ~ /^ [0-9]+ $/ {
    criterion = $2
    gsub(/ /, "", criterion)
    status = $3
    gsub(/^ +| +$/, "", status)
    print criterion > ids
    print criterion "\t" status > rows
  }
' ids="$tmpdir/map-ids" rows="$tmpdir/map-rows" "$acceptance"

sort -n "$tmpdir/map-ids" | uniq -d > "$tmpdir/duplicates"
if [[ -s "$tmpdir/duplicates" ]]; then
  echo "duplicate Zodex Local acceptance criterion mappings:" >&2
  cat "$tmpdir/duplicates" >&2
  exit 1
fi

sort -n "$tmpdir/plan-ids" > "$tmpdir/plan-sorted"
sort -n "$tmpdir/map-ids" > "$tmpdir/map-sorted"
if ! diff -u "$tmpdir/plan-sorted" "$tmpdir/map-sorted"; then
  echo "Zodex Local acceptance map does not match the current plan criteria" >&2
  exit 1
fi

if awk -F '\t' '$2 != "Zodex-proven" && $2 != "Phase-13-native" { print; bad = 1 } END { exit bad ? 0 : 1 }' "$tmpdir/map-rows" > "$tmpdir/bad-status"; then
  echo "Zodex Local acceptance map contains unsupported status values:" >&2
  cat "$tmpdir/bad-status" >&2
  exit 1
fi

# Keep the product-critical transport/routing contract explicit in normal CI
# on Linux and native macOS, even though the broad repository suite also
# contains these tests.
cargo test --lib \
  server::tests::modern_stateless_tool_call_observes_openai_session_without_transport_session \
  -- --exact
cargo test --lib \
  server::tests::tunnel_compat_initialize_is_sessionless_and_has_no_provider_attribution \
  -- --exact
cargo test --lib \
  server::tests::workdir_is_required_in_model_visible_exec_and_patch_schemas \
  -- --exact
cargo test --lib \
  server::local_tests::local_discovery_and_tools_list_are_stateless_and_runtime_specific \
  -- --exact
cargo test --test zodex_operator_cli \
  zodex_local_help_exposes_complete_public_family_and_inspection_examples \
  -- --exact
