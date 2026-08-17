#!/usr/bin/env bash
set -euo pipefail

# Keep the small model-facing transport contract, mode-first operator surface,
# and public documentation story executable in normal Linux/macOS CI. These
# checks deliberately derive from code/tests, not historical planning files.
cargo test --locked --lib \
  server::tests::modern_stateless_tool_call_observes_openai_session_without_transport_session \
  -- --exact
cargo test --locked --lib \
  server::tests::tunnel_compat_initialize_is_sessionless_and_has_no_provider_attribution \
  -- --exact
cargo test --locked --lib \
  server::tests::workdir_is_required_in_model_visible_exec_and_patch_schemas \
  -- --exact
cargo test --locked --lib \
  server::local_tests::local_discovery_and_tools_list_are_stateless_and_runtime_specific \
  -- --exact

cargo test --locked --test zodex_operator_cli \
  zodex_root_help_exposes_only_first_class_modes_and_upgrade \
  -- --exact
cargo test --locked --test zodex_operator_cli \
  zodex_root_rejects_removed_commands_and_global_config \
  -- --exact
cargo test --locked --test zodex_operator_cli \
  zodex_local_help_exposes_complete_public_family_and_inspection_examples \
  -- --exact

cargo test --locked --test docs_contract
