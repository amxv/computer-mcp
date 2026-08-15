use super::*;

pub(super) const OPERATOR_GUEST_MAX_TRANSFER_BYTES: usize = 1024 * 1024;

pub(super) trait OperatorGuestTransport {
    fn exec_privileged(&self, command: &[String]) -> Result<String>;
    fn write_file_atomic(&self, remote_path: &str, contents: &[u8]) -> Result<()>;
    fn identity_lines(&self) -> Vec<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OperatorGuestTarget {
    Sprite(ResolvedSprite),
    Local,
}

impl OperatorGuestTransport for OperatorGuestTarget {
    fn exec_privileged(&self, command: &[String]) -> Result<String> {
        if command.is_empty() {
            bail!("operator guest command must not be empty");
        }
        match self {
            Self::Sprite(resolved) => {
                run_sprite_exec(&resolved.name, resolved.org.as_deref(), command, &[])
            }
            Self::Local => run_local_machine_exec(command),
        }
    }

    fn write_file_atomic(&self, remote_path: &str, contents: &[u8]) -> Result<()> {
        validate_operator_guest_transfer(remote_path, contents)?;
        match self {
            Self::Sprite(resolved) => {
                let local =
                    NamedTempFile::new().context("failed to create operator guest temp file")?;
                fs::write(local.path(), contents)
                    .context("failed to stage operator guest upload")?;
                let mut rng = rand::rng();
                let suffix = Alphanumeric
                    .sample_string(&mut rng, 16)
                    .to_ascii_lowercase();
                let staging = format!("{remote_path}.zodex-upload-{suffix}");
                let command = sprite_atomic_move_command(&staging, remote_path);
                run_sprite_exec(
                    &resolved.name,
                    resolved.org.as_deref(),
                    &command,
                    &[(local.path(), staging.as_str())],
                )?;
                Ok(())
            }
            Self::Local => write_local_machine_file_atomic(remote_path, contents),
        }
    }

    fn identity_lines(&self) -> Vec<String> {
        match self {
            Self::Sprite(resolved) => {
                let mut lines = vec![format!("sprite: {}", resolved.name)];
                if let Some(org) = resolved.org.as_deref() {
                    lines.push(format!("org: {org}"));
                }
                lines
            }
            Self::Local => vec![format!("local: {LOCAL_MACHINE_NAME}")],
        }
    }
}

pub(super) fn validate_operator_guest_transfer(remote_path: &str, contents: &[u8]) -> Result<()> {
    if !remote_path.starts_with('/') || remote_path.contains(['\n', '\r', '\0']) {
        bail!("operator guest transfer path must be a safe absolute path");
    }
    if contents.len() > OPERATOR_GUEST_MAX_TRANSFER_BYTES {
        bail!(
            "operator guest transfer exceeds limit ({} bytes > {} bytes)",
            contents.len(),
            OPERATOR_GUEST_MAX_TRANSFER_BYTES
        );
    }
    Ok(())
}

pub(super) fn sprite_atomic_move_command(staging: &str, remote_path: &str) -> Vec<String> {
    vec![
        "/bin/sh".into(),
        "-c".into(),
        "set -eu; mv -f -- \"$1\" \"$2\"".into(),
        "zodex-upload".into(),
        staging.to_string(),
        remote_path.to_string(),
    ]
}

fn local_target_ready_for_github_mode(record: Option<&LocalTargetRecord>) -> bool {
    matches!(record, Some(record) if record.setup_state == LocalSetupState::Ready
        && record.machine_id == LOCAL_MACHINE_NAME
        && record.network.as_ref().is_some_and(local_network::local_network_expectation_matches))
}

pub(super) fn resolve_github_mode_target_from_state(
    explicit_local: bool,
    explicit_sprite: Option<&str>,
    explicit_org: Option<&str>,
    env_sprite: Option<&str>,
    registry: &OperatorSpriteRegistry,
    local_record: Option<&LocalTargetRecord>,
) -> Result<OperatorGuestTarget> {
    if explicit_local && explicit_sprite.is_some() {
        bail!("`--local` and `--sprite` are mutually exclusive");
    }
    if explicit_local {
        if explicit_org.is_some() {
            bail!("`--org` is Sprite-specific and cannot be used with `--local`");
        }
        if !local_target_ready_for_github_mode(local_record) {
            bail!("Local target is not ready; run `zodex local setup` first");
        }
        return Ok(OperatorGuestTarget::Local);
    }
    if explicit_sprite.is_some() {
        return resolve_remote_sprite_from_registry(explicit_sprite, explicit_org, None, registry)
            .map(OperatorGuestTarget::Sprite);
    }

    let env_sprite = env_sprite.filter(|value| !value.trim().is_empty());
    let sprite_candidates: Vec<ResolvedSprite> = if let Some(sprite) = env_sprite {
        vec![ResolvedSprite {
            name: sprite.to_string(),
            org: explicit_org.map(str::to_string),
        }]
    } else {
        registry
            .sprites
            .iter()
            .filter(|candidate| match explicit_org {
                Some(org) => candidate.org.as_deref() == Some(org),
                None => true,
            })
            .map(|candidate| ResolvedSprite {
                name: candidate.name.clone(),
                org: candidate.org.clone(),
            })
            .collect()
    };
    let local_eligible = explicit_org.is_none() && local_target_ready_for_github_mode(local_record);

    match (sprite_candidates.as_slice(), local_eligible) {
        ([], true) => Ok(OperatorGuestTarget::Local),
        ([sprite], false) => Ok(OperatorGuestTarget::Sprite(sprite.clone())),
        ([], false) => bail!(
            "no eligible Zodex target is configured; pass `--sprite <name>` or run `zodex sprite setup`, or run `zodex local setup` and pass `--local`"
        ),
        (_, true) => bail!(
            "Local and Sprite targets are both eligible; pass `--local` or `--sprite <name>` explicitly"
        ),
        (many, false) => {
            let names = many
                .iter()
                .map(|candidate| match candidate.org.as_deref() {
                    Some(org) => format!("{org}/{}", candidate.name),
                    None => candidate.name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            bail!("multiple Sprites are configured ({names}); pass `--sprite <name>` explicitly")
        }
    }
}

pub(super) fn resolve_github_mode_target(
    explicit_local: bool,
    explicit_sprite: Option<&str>,
    explicit_org: Option<&str>,
) -> Result<OperatorGuestTarget> {
    let env_sprite = env::var(ZODEX_SPRITE_ENV).ok();
    let needs_sprite_state = !explicit_local;
    let needs_local_state = explicit_local || (explicit_sprite.is_none() && explicit_org.is_none());
    let registry = if needs_sprite_state {
        load_operator_sprite_registry_from_path(&operator_sprites_registry_path()?)?
    } else {
        OperatorSpriteRegistry::default()
    };
    let local_record = if needs_local_state {
        let (target_path, _) = local_state_paths()?;
        load_local_target_record(&target_path)?
    } else {
        None
    };
    resolve_github_mode_target_from_state(
        explicit_local,
        explicit_sprite,
        explicit_org,
        env_sprite.as_deref(),
        &registry,
        local_record.as_ref(),
    )
}
