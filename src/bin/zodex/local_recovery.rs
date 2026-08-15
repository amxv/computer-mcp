use super::local_network::local_network_expectation_matches;
use super::local_setup::{
    LocalProvisioningFiles, LocalSetupAction, apply_local_provisioning,
    validate_local_default_base, validate_local_repo, validate_local_setup_file,
};
use super::local_tunnel::validate_local_tunnel_id;
use super::*;

pub(super) trait LocalResetRuntime {
    fn preflight_recreation(&mut self, intent: &LocalReadySetupIntent) -> Result<()>;
    fn revoke_access(&mut self) -> Result<()>;
    fn save_provisioning_state(&mut self, record: &LocalTargetRecord) -> Result<()>;
    fn delete_machine(&mut self) -> Result<()>;
    fn clear_lease(&mut self) -> Result<()>;
    fn reprovision(&mut self, record: LocalTargetRecord) -> Result<()>;
}

struct SystemLocalResetRuntime {
    target_path: PathBuf,
    lease_path: PathBuf,
    reader_pem: PathBuf,
    publisher_pem: PathBuf,
    tunnel_runtime_key: PathBuf,
}

impl LocalResetRuntime for SystemLocalResetRuntime {
    fn preflight_recreation(&mut self, _intent: &LocalReadySetupIntent) -> Result<()> {
        match probe_apple_provider() {
            LocalProviderAvailability::Ready { .. } => {}
            LocalProviderAvailability::Unsupported(reason) => {
                bail!("Local is unsupported: {reason}")
            }
            LocalProviderAvailability::Missing => bail!("Apple Container CLI is not installed"),
            LocalProviderAvailability::Incompatible(reason) => {
                bail!("Apple Container is incompatible: {reason}")
            }
        }
        ensure_apple_container_system_started()?;
        build_local_machine_image().context("failed to prebuild Local reset machine image")?;
        Ok(())
    }

    fn revoke_access(&mut self) -> Result<()> {
        super::local_lifecycle::local_revoke_access_before_reset()
    }

    fn save_provisioning_state(&mut self, record: &LocalTargetRecord) -> Result<()> {
        save_local_target_record(&self.target_path, record)
    }

    fn delete_machine(&mut self) -> Result<()> {
        delete_local_machine().context("failed to delete the existing Local machine")
    }

    fn clear_lease(&mut self) -> Result<()> {
        remove_local_access_lease(&self.lease_path)
    }

    fn reprovision(&mut self, record: LocalTargetRecord) -> Result<()> {
        apply_local_provisioning(
            &self.target_path,
            record,
            LocalSetupAction::Create,
            None,
            LocalProvisioningFiles {
                reader_pem: &self.reader_pem,
                publisher_pem: &self.publisher_pem,
                tunnel_runtime_key: &self.tunnel_runtime_key,
            },
            true,
        )?;
        Ok(())
    }
}

pub(super) fn resolve_local_reset_intent(
    target: Option<&LocalTargetRecord>,
    saved: Option<LocalReadySetupIntent>,
) -> Result<LocalReadySetupIntent> {
    if let Some(saved) = saved {
        return Ok(saved);
    }
    let target = target.ok_or_else(|| {
        anyhow!("Zodex Local is not configured; run `zodex local setup` before reset")
    })?;
    if target.setup_state != LocalSetupState::Ready {
        bail!(
            "Local setup is incomplete and no last-ready recreation intent exists; rerun `zodex local setup` before reset"
        );
    }
    local_ready_setup_intent_from_target(target)
}

pub(super) fn validate_local_reset_intent(intent: &LocalReadySetupIntent) -> Result<()> {
    if intent.machine_id != LOCAL_MACHINE_NAME {
        bail!("saved Local setup targets the wrong machine identity");
    }
    if !local_network_expectation_matches(&intent.network) {
        bail!(
            "saved Local setup network policy predates this Zodex build; rerun `zodex local setup` before reset"
        );
    }
    if intent.image_reference != LOCAL_MACHINE_IMAGE {
        bail!(
            "saved Local setup uses machine image `{}` but this Zodex build expects `{LOCAL_MACHINE_IMAGE}`; rerun `zodex local setup` before reset",
            intent.image_reference
        );
    }
    validate_local_repo(&intent.setup_sources.repo)?;
    validate_local_default_base(&intent.setup_sources.default_base)?;
    let tunnel_id = intent
        .setup_sources
        .tunnel_id
        .as_deref()
        .ok_or_else(|| anyhow!("saved Local setup is missing its tunnel ID"))?;
    validate_local_tunnel_id(tunnel_id)?;

    let reader_pem = Path::new(&intent.setup_sources.reader_pem_path);
    let publisher_pem = Path::new(&intent.setup_sources.publisher_pem_path);
    let tunnel_runtime_key = Path::new(
        intent
            .setup_sources
            .tunnel_runtime_key_path
            .as_deref()
            .ok_or_else(|| {
                anyhow!("saved Local setup is missing its tunnel runtime-key source path")
            })?,
    );
    validate_local_setup_file(reader_pem, "saved reader PEM")?;
    validate_local_setup_file(publisher_pem, "saved publisher PEM")?;
    validate_local_setup_file(tunnel_runtime_key, "saved tunnel runtime key")?;
    if fs::metadata(tunnel_runtime_key)
        .with_context(|| {
            format!(
                "failed to inspect saved tunnel runtime key at {}",
                tunnel_runtime_key.display()
            )
        })?
        .len()
        == 0
    {
        bail!("saved tunnel runtime key file must not be empty");
    }
    if matches!(intent.requested_cpus, Some(0)) {
        bail!("saved Local CPU count must be greater than zero");
    }
    if let Some(memory) = intent.requested_memory.as_deref() {
        parse_local_memory_bytes(memory)?;
    }
    Ok(())
}

async fn preflight_local_reset_github(intent: &LocalReadySetupIntent) -> Result<()> {
    let sources = &intent.setup_sources;
    let reader_pem = Path::new(&sources.reader_pem_path);
    let publisher_pem = Path::new(&sources.publisher_pem_path);
    mint_reader_installation_token(
        sources.reader_app_id,
        reader_pem,
        sources.reader_installation_id,
    )
    .await
    .context("failed to validate saved reader GitHub authority before reset")?;
    mint_publisher_installation_token_with_metadata(
        sources.publisher_app_id,
        publisher_pem,
        sources.publisher_installation_id,
    )
    .await
    .context("failed to validate saved publisher GitHub authority before reset")?;
    Ok(())
}

pub(super) fn reset_local_with_runtime<R: LocalResetRuntime>(
    runtime: &mut R,
    intent: &LocalReadySetupIntent,
) -> Result<()> {
    runtime.preflight_recreation(intent)?;
    runtime.revoke_access()?;

    let provisioning = local_target_record_from_ready_intent(intent, LocalSetupState::Provisioning);
    runtime.save_provisioning_state(&provisioning)?;
    runtime.clear_lease()?;
    runtime.delete_machine()?;
    runtime.reprovision(provisioning)?;
    Ok(())
}

pub(super) async fn local_reset() -> Result<()> {
    let (target_path, lease_path) = local_state_paths()?;
    let target = load_local_target_record(&target_path)?;
    let saved_path = local_last_ready_setup_path()?;
    let saved = load_local_ready_setup_intent(&saved_path)?;
    let intent = resolve_local_reset_intent(target.as_ref(), saved)?;

    validate_local_reset_intent(&intent)
        .context("Local reset preflight failed; the existing machine was not stopped or deleted")?;
    preflight_local_reset_github(&intent)
        .await
        .context("Local reset preflight failed; the existing machine was not stopped or deleted")?;

    let mut runtime = SystemLocalResetRuntime {
        target_path,
        lease_path,
        reader_pem: PathBuf::from(&intent.setup_sources.reader_pem_path),
        publisher_pem: PathBuf::from(&intent.setup_sources.publisher_pem_path),
        tunnel_runtime_key: PathBuf::from(
            intent
                .setup_sources
                .tunnel_runtime_key_path
                .as_deref()
                .expect("validated tunnel runtime-key path"),
        ),
    };
    reset_local_with_runtime(&mut runtime, &intent)?;
    println!("local-reset: complete");
    println!("MCP access: inactive (run `zodex local start --ttl <duration>` to reconnect)");
    Ok(())
}
