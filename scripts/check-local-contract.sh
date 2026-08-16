#!/usr/bin/env bash
set -euo pipefail

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
