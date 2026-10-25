use super::*;
use crate::{gp_params::GpParams, session::desktop::network::GatewayBinding};
use reqwest::redirect::Policy;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl GatewayTransport {
  pub fn new(
    server: &str,
    authentication: GatewayAuthentication,
    params: &GpParams,
    binding: Option<&GatewayBinding>,
  ) -> anyhow::Result<Self> {
    let origin = Url::parse(&crate::utils::normalize_server(server)?)?;
    let client = match binding {
      Some(binding) => bound_client(server, params, binding)?,
      None => Client::try_from(params)?,
    };
    Ok(Self {
      client,
      origin,
      authentication,
      profile: params.os_profile().clone(),
    })
  }
}
pub(crate) fn bound_client(server: &str, params: &GpParams, binding: &GatewayBinding) -> anyhow::Result<Client> {
  binding.validate_server(server)?;
  let origin = gateway_origin(server)?;
  let host = origin
    .host_str()
    .ok_or_else(|| anyhow::anyhow!("Gateway host is missing"))?;
  let builder = params
    .client_builder()?
    .redirect(Policy::none())
    .no_proxy()
    .local_address(binding.source().ip())
    .resolve(host, binding.endpoint())
    .timeout(REQUEST_TIMEOUT);
  let builder = bind_interface(builder, binding)?;
  Ok(builder.build()?)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn bind_interface(builder: reqwest::ClientBuilder, binding: &GatewayBinding) -> anyhow::Result<reqwest::ClientBuilder> {
  Ok(builder.interface(binding.interface()))
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn bind_interface(builder: reqwest::ClientBuilder, binding: &GatewayBinding) -> anyhow::Result<reqwest::ClientBuilder> {
  if let std::net::SocketAddr::V6(source) = binding.source() {
    anyhow::ensure!(
      source.scope_id() == 0,
      "Scoped IPv6 source requires a native socket connector"
    );
  }
  Ok(builder)
}

pub(crate) fn gateway_origin(server: &str) -> anyhow::Result<Url> {
  let server = if server.starts_with("https://") {
    server.to_owned()
  } else {
    format!("https://{server}")
  };
  let origin = Url::parse(&server)?;
  anyhow::ensure!(
    origin.scheme() == "https"
      && origin.username().is_empty()
      && origin.password().is_none()
      && origin.path() == "/"
      && origin.query().is_none()
      && origin.fragment().is_none(),
    "Gateway endpoint must be an HTTPS origin"
  );
  Ok(origin)
}
