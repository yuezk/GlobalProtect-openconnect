use std::path::PathBuf;
use log::{info, warn};

use crate::gp_params::GpParams;
use crate::cookie_store;
use crate::cookie_store::StoredCookie;
use crate::credential::Credential;
use crate::gateway::{GatewayLogin, gateway_login};

pub struct CachedAuthOptions {
  pub path: PathBuf,
}

pub struct CachedAuth {
  pub stored: StoredCookie,
  pub gateway_cookie: String,
}

pub async fn try_cached_path(
  options: &CachedAuthOptions,
  server: &str,
  gp_params: &GpParams,
) -> anyhow::Result<Option<CachedAuth>> {
  if gp_params.is_gateway() {
    info!("Not a gateway");
    return Ok(None);
  }

  let host_id = gp_params.os_profile().host_identity().host_id();
  let Some(stored) = cookie_store::load(&options.path, server, &host_id) else {
    info!("No cookie stored in {:?}", &options.path);
    return Ok(None);
  };

  if !stored.auth_cookie.can_authenticate_gateway() {
    warn!(
      "Cached portal cookie for {} is not usable for gateway authentication. Clearing cache.",
      stored.server
    );
    cookie_store::clear(&options.path);
    return Ok(None);
  }

  let credential: Credential = (&stored.auth_cookie).into();
  let gateway_params = gp_params.as_gateway();

  info!(
    "Using cached portal cookie for {} (saved_at={}, gateway={})",
    stored.server, stored.saved_at, stored.last_gateway
  );

  let login_result = gateway_login(&stored.last_gateway, &credential, &gateway_params).await;

  match login_result {
    Ok(GatewayLogin::Cookie(gateway_cookie)) => Ok(Some(CachedAuth{
      stored,
      gateway_cookie,
    })),
    Ok(GatewayLogin::Mfa(_, _)) => {
      warn!(
        "Cached portal cookie for {} cannot automatically answer MFA",
        stored.last_gateway
      );
      cookie_store::clear(&options.path);
      Ok(None)
    }
    Err(err) => {
      warn!(
        "Cached portal cookie rejected by gateway {}: {}",
        stored.last_gateway, err
      );
      cookie_store::clear(&options.path);
      Ok(None)
    }
  }
}
