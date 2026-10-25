//! Desktop portal discovery: protocol retrieval followed by native location detection.
use super::{PortalConfig, retrieve_config};
use crate::{credential::Credential, gp_params::GpParams};
use tokio_util::sync::CancellationToken;

/// Do not drop this future on disconnect: native detection must cancel and join.
pub async fn discover(
  portal: &str,
  credential: &Credential,
  params: &GpParams,
  cancellation: &CancellationToken,
) -> anyhow::Result<PortalConfig> {
  let policy = retrieve_config(portal, credential, params, cancellation).await?;
  let detected = match policy.detection() {
    Some(detection) => Some(
      crate::session::desktop::network::detect(detection, cancellation)
        .await
        .unwrap_or_else(|error| {
          log::warn!("Internal host detection failed: {error}");
          false
        }),
    ),
    None => None,
  };
  if cancellation.is_cancelled() {
    anyhow::bail!("Portal configuration cancelled");
  }
  Ok(policy.select_gateways(detected.unwrap_or(false), detected))
}
