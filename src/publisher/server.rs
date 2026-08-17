use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use tempfile::{TempDir, tempdir};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::config::{Config, PublishTarget};

use super::api::{
    DirectPushRequest, DirectPushResponse, PublishPrError, PublishPrRequest, PublishPrResponse,
    PublisherRequest, PublisherResponse,
};
use super::github::{
    clone_repo_with_token, create_pull_request, git_plain, git_with_token, github_repo_https_url,
    mint_publisher_installation_token, write_askpass_script,
};
use super::validation::{
    build_publish_branch_name, validate_direct_push_request, validate_git_object_id,
    validate_publish_request, validate_publisher_config,
};
use super::{
    DIRECT_PUSH_IMPORTED_REF, IMPORTED_REF, MAX_PUBLISHER_METADATA_BYTES,
    MAX_PUBLISHER_RESPONSE_BYTES, PUBLISHER_STREAM_BUFFER_BYTES, PUBLISHER_WIRE_HEADER_BYTES,
    PUBLISHER_WIRE_MAGIC, PUBLISHER_WIRE_VERSION, SOCKET_DIR_MODE, SOCKET_MODE,
};

const PUBLISHER_METADATA_ACCEPTED: u8 = 1;
const PUBLISHER_METADATA_REJECTED: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PublisherWireHeader {
    pub(super) metadata_len: usize,
    pub(super) bundle_len: u64,
}

#[derive(Debug)]
pub(super) struct ReceivedBundle {
    _tempdir: TempDir,
    path: PathBuf,
}

impl ReceivedBundle {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
pub(super) enum ValidatedPublisherRequest {
    PublishPr {
        request: PublishPrRequest,
        target: PublishTarget,
        bundle_len: u64,
    },
    DirectPush {
        request: DirectPushRequest,
        target: PublishTarget,
        bundle_len: u64,
    },
}

pub(super) fn ensure_publisher_socket_parent_dir(socket_path: &Path) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create publisher socket directory {}",
                parent.display()
            )
        })?;
        fs::set_permissions(parent, fs::Permissions::from_mode(SOCKET_DIR_MODE))
            .with_context(|| format!("failed to chmod {}", parent.display()))?;
    }

    Ok(())
}

pub async fn serve_publisher(config: Config) -> Result<()> {
    validate_publisher_config(&config)?;

    let socket_path = Path::new(&config.publisher_socket_path);
    ensure_publisher_socket_parent_dir(socket_path)?;

    if socket_path.exists() {
        fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    }

    let listener = UnixListener::bind(socket_path).with_context(|| {
        format!(
            "failed to bind publisher socket at {}",
            socket_path.display()
        )
    })?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(SOCKET_MODE))
        .with_context(|| format!("failed to chmod {}", socket_path.display()))?;

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .context("failed to accept publisher connection")?;
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, config).await {
                tracing::error!(error = %err, "publisher request failed");
            }
        });
    }
}

pub async fn submit_publish_request(
    socket_path: &Path,
    max_bundle_bytes: usize,
    request: &PublishPrRequest,
    bundle_path: &Path,
) -> Result<PublishPrResponse> {
    let response = submit_publisher_request_frame(
        socket_path,
        max_bundle_bytes,
        &PublisherRequest::PublishPr(request.clone()),
        Some(bundle_path),
    )
    .await?;
    match serde_json::from_slice::<PublisherResponse>(&response) {
        Ok(PublisherResponse::PublishPr(response)) => Ok(response),
        Ok(PublisherResponse::DirectPush(_)) => {
            bail!("publisher returned unexpected response type")
        }
        Err(_) => serde_json::from_slice(&response).context("failed to decode publish response"),
    }
}

pub async fn submit_direct_push_request(
    socket_path: &Path,
    max_bundle_bytes: usize,
    request: &DirectPushRequest,
    bundle_path: Option<&Path>,
) -> Result<DirectPushResponse> {
    let response = submit_publisher_request_frame(
        socket_path,
        max_bundle_bytes,
        &PublisherRequest::DirectPush(request.clone()),
        bundle_path,
    )
    .await?;
    match serde_json::from_slice::<PublisherResponse>(&response) {
        Ok(PublisherResponse::DirectPush(response)) => Ok(response),
        Ok(PublisherResponse::PublishPr(_)) => bail!("publisher returned unexpected response type"),
        Err(_) => {
            serde_json::from_slice(&response).context("failed to decode direct push response")
        }
    }
}

pub(super) fn encode_publisher_wire_header(
    metadata_len: usize,
    bundle_len: u64,
) -> Result<[u8; PUBLISHER_WIRE_HEADER_BYTES]> {
    if metadata_len == 0 {
        bail!("publisher metadata cannot be empty");
    }
    if metadata_len > MAX_PUBLISHER_METADATA_BYTES {
        bail!(
            "publisher metadata exceeds local limit ({} bytes > {} bytes)",
            metadata_len,
            MAX_PUBLISHER_METADATA_BYTES
        );
    }
    let metadata_len = u32::try_from(metadata_len).context("publisher metadata length overflow")?;
    let mut header = [0u8; PUBLISHER_WIRE_HEADER_BYTES];
    header[..8].copy_from_slice(&PUBLISHER_WIRE_MAGIC);
    header[8..10].copy_from_slice(&PUBLISHER_WIRE_VERSION.to_be_bytes());
    header[10..12].copy_from_slice(&0u16.to_be_bytes());
    header[12..16].copy_from_slice(&metadata_len.to_be_bytes());
    header[16..24].copy_from_slice(&bundle_len.to_be_bytes());
    Ok(header)
}

pub(super) fn decode_publisher_wire_header(
    header: &[u8; PUBLISHER_WIRE_HEADER_BYTES],
) -> Result<PublisherWireHeader> {
    if header[..8] != PUBLISHER_WIRE_MAGIC {
        bail!("publisher request has an invalid wire magic");
    }
    let version = u16::from_be_bytes([header[8], header[9]]);
    if version != PUBLISHER_WIRE_VERSION {
        bail!("unsupported publisher wire version {version}; expected {PUBLISHER_WIRE_VERSION}");
    }
    let flags = u16::from_be_bytes([header[10], header[11]]);
    if flags != 0 {
        bail!("publisher request has unsupported wire flags: {flags}");
    }
    let metadata_len =
        u32::from_be_bytes([header[12], header[13], header[14], header[15]]) as usize;
    if metadata_len == 0 {
        bail!("publisher metadata cannot be empty");
    }
    if metadata_len > MAX_PUBLISHER_METADATA_BYTES {
        bail!(
            "publisher metadata exceeds local limit ({} bytes > {} bytes)",
            metadata_len,
            MAX_PUBLISHER_METADATA_BYTES
        );
    }
    let bundle_len = u64::from_be_bytes([
        header[16], header[17], header[18], header[19], header[20], header[21], header[22],
        header[23],
    ]);
    Ok(PublisherWireHeader {
        metadata_len,
        bundle_len,
    })
}

pub(super) fn encode_publisher_metadata(request: &PublisherRequest) -> Result<Vec<u8>> {
    let metadata = serde_json::to_vec(request).context("failed to serialize publisher metadata")?;
    if metadata.is_empty() || metadata.len() > MAX_PUBLISHER_METADATA_BYTES {
        bail!(
            "publisher metadata exceeds local limit ({} bytes > {} bytes)",
            metadata.len(),
            MAX_PUBLISHER_METADATA_BYTES
        );
    }
    Ok(metadata)
}

pub(super) fn decode_publisher_metadata(metadata: &[u8]) -> Result<PublisherRequest> {
    if metadata.is_empty() {
        bail!("publisher metadata cannot be empty");
    }
    if metadata.len() > MAX_PUBLISHER_METADATA_BYTES {
        bail!("publisher metadata exceeds local limit");
    }
    serde_json::from_slice(metadata).context("publisher metadata was not valid request JSON")
}

async fn submit_publisher_request_frame(
    socket_path: &Path,
    max_bundle_bytes: usize,
    request: &PublisherRequest,
    bundle_path: Option<&Path>,
) -> Result<Vec<u8>> {
    let metadata = encode_publisher_metadata(request)?;
    let bundle_len = match bundle_path {
        Some(path) => tokio::fs::metadata(path)
            .await
            .with_context(|| format!("failed to stat publisher bundle {}", path.display()))?
            .len(),
        None => 0,
    };
    if bundle_len > max_bundle_bytes as u64 {
        bail!(
            "publisher bundle exceeds configured limit ({} bytes > {} bytes)",
            bundle_len,
            max_bundle_bytes
        );
    }
    let header = encode_publisher_wire_header(metadata.len(), bundle_len)?;

    let mut stream = UnixStream::connect(socket_path).await.with_context(|| {
        format!(
            "failed to connect to publisher socket {}",
            socket_path.display()
        )
    })?;
    stream
        .write_all(&header)
        .await
        .context("failed to write publisher frame header")?;
    stream
        .write_all(&metadata)
        .await
        .context("failed to write publisher metadata")?;
    stream
        .flush()
        .await
        .context("failed to flush publisher metadata")?;

    match read_publisher_metadata_ack(&mut stream).await? {
        None => {}
        Some(error) => bail!(error),
    }

    if let Some(path) = bundle_path {
        stream_bundle_file(&mut stream, path, bundle_len).await?;
    }
    stream
        .shutdown()
        .await
        .context("failed to close publisher request stream")?;

    let mut response_buf = Vec::new();
    (&mut stream)
        .take((MAX_PUBLISHER_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response_buf)
        .await
        .context("failed to read publisher response")?;
    if response_buf.is_empty() {
        bail!("publisher returned an empty response");
    }
    if response_buf.len() > MAX_PUBLISHER_RESPONSE_BYTES {
        bail!("publisher response exceeds local size limit");
    }

    if let Ok(error) = serde_json::from_slice::<PublishPrError>(&response_buf) {
        bail!(error.error);
    }

    Ok(response_buf)
}

async fn stream_bundle_file(stream: &mut UnixStream, path: &Path, declared_len: u64) -> Result<()> {
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("failed to open publisher bundle {}", path.display()))?;
    let mut limited = (&mut file).take(declared_len);
    let copied = tokio::io::copy(&mut limited, stream)
        .await
        .context("failed to stream publisher bundle")?;
    if copied != declared_len {
        bail!(
            "publisher bundle changed while sending ({} bytes streamed, {} declared)",
            copied,
            declared_len
        );
    }
    let mut extra = [0u8; 1];
    if file
        .read(&mut extra)
        .await
        .context("failed to verify publisher bundle length")?
        != 0
    {
        bail!("publisher bundle grew beyond its declared length while sending");
    }
    Ok(())
}

async fn read_publisher_metadata_ack(stream: &mut UnixStream) -> Result<Option<String>> {
    let mut status = [0u8; 1];
    stream
        .read_exact(&mut status)
        .await
        .context("failed to read publisher metadata acknowledgement")?;
    match status[0] {
        PUBLISHER_METADATA_ACCEPTED => Ok(None),
        PUBLISHER_METADATA_REJECTED => {
            let mut len = [0u8; 4];
            stream
                .read_exact(&mut len)
                .await
                .context("failed to read publisher metadata rejection length")?;
            let len = u32::from_be_bytes(len) as usize;
            if len == 0 || len > MAX_PUBLISHER_RESPONSE_BYTES {
                bail!("publisher returned an invalid metadata rejection frame");
            }
            let mut payload = vec![0u8; len];
            stream
                .read_exact(&mut payload)
                .await
                .context("failed to read publisher metadata rejection")?;
            let error: PublishPrError = serde_json::from_slice(&payload)
                .context("failed to decode publisher metadata rejection")?;
            Ok(Some(error.error))
        }
        other => bail!("publisher returned unknown metadata acknowledgement byte {other}"),
    }
}

async fn handle_connection(mut stream: UnixStream, config: Config) -> Result<()> {
    let validated = match read_and_validate_publisher_metadata(&mut stream, &config).await {
        Ok(validated) => validated,
        Err(err) => {
            write_publisher_metadata_rejection(&mut stream, &err).await?;
            return Ok(());
        }
    };
    stream
        .write_all(&[PUBLISHER_METADATA_ACCEPTED])
        .await
        .context("failed to acknowledge publisher metadata")?;
    stream
        .flush()
        .await
        .context("failed to flush publisher metadata acknowledgement")?;

    let bundle_len = validated_bundle_len(&validated);
    let received_bundle = match receive_declared_bundle(&mut stream, bundle_len).await {
        Ok(bundle) => bundle,
        Err(err) => {
            let response = encode_publisher_error("publisher bundle receive", &err)?;
            write_final_publisher_response(&mut stream, &response).await?;
            return Ok(());
        }
    };

    let response = match validated {
        ValidatedPublisherRequest::PublishPr {
            request, target, ..
        } => {
            let bundle = received_bundle
                .as_ref()
                .ok_or_else(|| anyhow!("publish-pr bundle was missing after validation"))?;
            match handle_publish_request(&config, request, &target, bundle.path()).await {
                Ok(response) => serde_json::to_vec(&PublisherResponse::PublishPr(response))
                    .context("failed to encode publish response")?,
                Err(err) => encode_publisher_error("publish-pr", &err)?,
            }
        }
        ValidatedPublisherRequest::DirectPush {
            request, target, ..
        } => {
            match handle_direct_push_request(
                &config,
                request,
                &target,
                received_bundle.as_ref().map(ReceivedBundle::path),
            )
            .await
            {
                Ok(response) => serde_json::to_vec(&PublisherResponse::DirectPush(response))
                    .context("failed to encode direct push response")?,
                Err(err) => encode_publisher_error("direct push", &err)?,
            }
        }
    };

    write_final_publisher_response(&mut stream, &response).await
}

fn encode_publisher_error(operation: &str, err: &anyhow::Error) -> Result<Vec<u8>> {
    let error = error_chain_string(err);
    tracing::error!(operation, error = %error, "publisher operation failed");
    serde_json::to_vec(&PublishPrError { error })
        .context("failed to encode publisher error response")
}

fn error_chain_string(err: &anyhow::Error) -> String {
    err.chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

async fn read_and_validate_publisher_metadata(
    stream: &mut UnixStream,
    config: &Config,
) -> Result<ValidatedPublisherRequest> {
    let mut header_bytes = [0u8; PUBLISHER_WIRE_HEADER_BYTES];
    stream
        .read_exact(&mut header_bytes)
        .await
        .context("failed to read publisher frame header")?;
    let header = decode_publisher_wire_header(&header_bytes)?;
    if header.bundle_len > config.publisher_max_bundle_bytes as u64 {
        bail!(
            "publisher bundle exceeds configured limit ({} bytes > {} bytes)",
            header.bundle_len,
            config.publisher_max_bundle_bytes
        );
    }

    let mut metadata = vec![0u8; header.metadata_len];
    stream
        .read_exact(&mut metadata)
        .await
        .context("failed to read publisher metadata")?;
    let request = decode_publisher_metadata(&metadata)?;
    validate_publisher_request_before_body(config, request, header.bundle_len)
}

pub(super) fn validate_publisher_request_before_body(
    config: &Config,
    request: PublisherRequest,
    bundle_len: u64,
) -> Result<ValidatedPublisherRequest> {
    if bundle_len > config.publisher_max_bundle_bytes as u64 {
        bail!(
            "publisher bundle exceeds configured limit ({} bytes > {} bytes)",
            bundle_len,
            config.publisher_max_bundle_bytes
        );
    }

    match request {
        PublisherRequest::PublishPr(request) => {
            if bundle_len == 0 {
                bail!("publish bundle cannot be empty");
            }
            let target = validate_publish_request(config, &request)?;
            Ok(ValidatedPublisherRequest::PublishPr {
                request,
                target,
                bundle_len,
            })
        }
        PublisherRequest::DirectPush(request) => {
            if request.src.is_empty() && bundle_len != 0 {
                bail!("direct push deletion request cannot include a bundle");
            }
            if !request.src.is_empty() && bundle_len == 0 && request.src_oid.is_none() {
                bail!(
                    "direct push bundle is empty and no source object id was provided; this usually means the pushed ref points to an object already present on the remote"
                );
            }
            let target = validate_direct_push_request(config, &request)?;
            Ok(ValidatedPublisherRequest::DirectPush {
                request,
                target,
                bundle_len,
            })
        }
    }
}

fn validated_bundle_len(request: &ValidatedPublisherRequest) -> u64 {
    match request {
        ValidatedPublisherRequest::PublishPr { bundle_len, .. }
        | ValidatedPublisherRequest::DirectPush { bundle_len, .. } => *bundle_len,
    }
}

async fn write_publisher_metadata_rejection(
    stream: &mut UnixStream,
    err: &anyhow::Error,
) -> Result<()> {
    let payload = encode_publisher_error("publisher metadata validation", err)?;
    if payload.len() > MAX_PUBLISHER_RESPONSE_BYTES {
        bail!("publisher metadata rejection exceeds response limit");
    }
    stream
        .write_all(&[PUBLISHER_METADATA_REJECTED])
        .await
        .context("failed to write publisher metadata rejection status")?;
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .context("failed to write publisher metadata rejection length")?;
    stream
        .write_all(&payload)
        .await
        .context("failed to write publisher metadata rejection")?;
    stream
        .shutdown()
        .await
        .context("failed to close rejected publisher request")?;
    Ok(())
}

async fn write_final_publisher_response(stream: &mut UnixStream, response: &[u8]) -> Result<()> {
    if response.is_empty() || response.len() > MAX_PUBLISHER_RESPONSE_BYTES {
        bail!("publisher response is outside the allowed size range");
    }
    stream
        .write_all(response)
        .await
        .context("failed to write publisher response")?;
    stream
        .shutdown()
        .await
        .context("failed to close publisher response stream")?;
    Ok(())
}

pub(super) async fn receive_declared_bundle(
    stream: &mut UnixStream,
    bundle_len: u64,
) -> Result<Option<ReceivedBundle>> {
    if bundle_len == 0 {
        let mut extra = [0u8; 1];
        if stream
            .read(&mut extra)
            .await
            .context("failed to verify empty publisher bundle")?
            != 0
        {
            bail!("publisher peer sent bytes beyond declared bundle length");
        }
        return Ok(None);
    }

    let tempdir = tempdir().context("failed to create publisher bundle tempdir")?;
    let bundle_path = tempdir.path().join("request.bundle");
    let mut file = tokio::fs::File::create(&bundle_path)
        .await
        .with_context(|| format!("failed to create {}", bundle_path.display()))?;
    let mut remaining = bundle_len;
    let mut buffer = vec![0u8; PUBLISHER_STREAM_BUFFER_BYTES];
    while remaining != 0 {
        let chunk_len = remaining.min(PUBLISHER_STREAM_BUFFER_BYTES as u64) as usize;
        let read = stream
            .read(&mut buffer[..chunk_len])
            .await
            .context("failed while reading publisher bundle")?;
        if read == 0 {
            bail!("publisher bundle ended early ({} bytes missing)", remaining);
        }
        file.write_all(&buffer[..read])
            .await
            .context("failed while spooling publisher bundle")?;
        remaining -= read as u64;
    }
    file.flush()
        .await
        .context("failed to flush publisher bundle")?;
    drop(file);

    let mut extra = [0u8; 1];
    if stream
        .read(&mut extra)
        .await
        .context("failed to verify publisher bundle framing")?
        != 0
    {
        bail!("publisher peer sent bytes beyond declared bundle length");
    }

    Ok(Some(ReceivedBundle {
        _tempdir: tempdir,
        path: bundle_path,
    }))
}

async fn handle_direct_push_request(
    config: &Config,
    request: DirectPushRequest,
    target: &PublishTarget,
    bundle_path: Option<&Path>,
) -> Result<DirectPushResponse> {
    let token = mint_publisher_installation_token(
        config
            .publisher_app_id
            .ok_or_else(|| anyhow!("publisher_app_id is not configured"))?,
        Path::new(&config.publisher_private_key_path),
        target.installation_id,
    )
    .await?;

    let tempdir = tempdir().context("failed to create publisher tempdir")?;
    let askpass_path = write_askpass_script(tempdir.path())?;
    let repo_dir = clone_repo_with_token(tempdir.path(), &token, &askpass_path, &target.repo)?;

    if request.src.is_empty() {
        git_with_token(
            &repo_dir,
            &token,
            &askpass_path,
            &[
                "push",
                &github_repo_https_url(&target.repo),
                &format!(":{}", request.dst),
            ],
        )?;
    } else {
        let push_src = if let Some(bundle_path) = bundle_path {
            git_plain(
                &repo_dir,
                &[
                    "fetch",
                    bundle_path.to_str().unwrap(),
                    &format!("{}:{DIRECT_PUSH_IMPORTED_REF}", request.src),
                ],
            )?;
            DIRECT_PUSH_IMPORTED_REF.to_string()
        } else {
            let src_oid = request.src_oid.as_deref().ok_or_else(|| {
                anyhow!(
                    "direct push bundle is empty and no source object id was provided; this usually means the pushed ref points to an object already present on the remote"
                )
            })?;
            validate_git_object_id(src_oid)?;
            git_plain(&repo_dir, &["cat-file", "-e", &format!("{src_oid}^{{object}}")])
                .with_context(|| {
                    format!(
                        "direct push source object {src_oid} is not present in the publisher clone; retry after pushing the containing branch or include the object in the push bundle"
                    )
                })?;
            src_oid.to_string()
        };
        let refspec = if request.force {
            format!("+{push_src}:{}", request.dst)
        } else {
            format!("{push_src}:{}", request.dst)
        };
        git_with_token(
            &repo_dir,
            &token,
            &askpass_path,
            &["push", &github_repo_https_url(&target.repo), &refspec],
        )?;
    }

    Ok(DirectPushResponse {
        repo: target.repo.clone(),
        dst: request.dst,
    })
}

async fn handle_publish_request(
    config: &Config,
    request: PublishPrRequest,
    target: &PublishTarget,
    bundle_path: &Path,
) -> Result<PublishPrResponse> {
    let token = mint_publisher_installation_token(
        config
            .publisher_app_id
            .ok_or_else(|| anyhow!("publisher_app_id is not configured"))?,
        Path::new(&config.publisher_private_key_path),
        target.installation_id,
    )
    .await?;

    let tempdir = tempdir().context("failed to create publisher tempdir")?;
    let askpass_path = write_askpass_script(tempdir.path())?;

    let repo_dir = tempdir.path().join("repo");
    git_with_token(
        tempdir.path(),
        &token,
        &askpass_path,
        &[
            "clone",
            "--quiet",
            &github_repo_https_url(&target.repo),
            repo_dir.to_str().unwrap(),
        ],
    )?;

    git_plain(
        &repo_dir,
        &[
            "fetch",
            bundle_path.to_str().unwrap(),
            &format!("HEAD:{IMPORTED_REF}"),
        ],
    )?;

    let branch = build_publish_branch_name(&config.publisher_branch_prefix);
    git_plain(&repo_dir, &["checkout", "-B", &branch, IMPORTED_REF])?;
    git_with_token(
        &repo_dir,
        &token,
        &askpass_path,
        &["push", "origin", &format!("HEAD:refs/heads/{branch}")],
    )?;

    let pr = create_pull_request(
        &token,
        &target.repo,
        request.base.as_deref().unwrap_or(&target.default_base),
        &branch,
        &request.title,
        &request.body,
        request.draft,
    )
    .await?;

    Ok(pr)
}
