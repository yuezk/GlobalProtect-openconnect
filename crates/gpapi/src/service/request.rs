use std::{collections::HashMap, fmt};

use serde::{Deserialize, Serialize};
use specta::Type;
use zeroize::Zeroize;

use crate::{
  hip::HipSource,
  os_profile::{ClientOs, HostIdentity, OsProfile},
  session::ConnectionPlan,
};

use super::vpn_state::ConnectInfo;

pub const MAX_CLIENT_IDENTITY_DATA: usize = 32 * 1024;

#[derive(Debug, Deserialize, Serialize)]
pub struct StoreEditedHipReportChunkRequest {
  pub upload_id: Option<String>,
  pub offset: usize,
  pub xml: String,
  pub complete: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PreviewHipReportRequest {
  pub source: HipSource,
  pub client_os: ClientOs,
  pub client_version: String,
  pub host_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReadHipPreviewChunkRequest {
  pub preview_id: String,
  pub offset: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LaunchGuiRequest {
  user: String,
  envs: HashMap<String, String>,
}

impl LaunchGuiRequest {
  pub fn new(user: String, envs: HashMap<String, String>) -> Self {
    Self { user, envs }
  }

  pub fn user(&self) -> &str {
    &self.user
  }

  pub fn envs(&self) -> &HashMap<String, String> {
    &self.envs
  }
}

#[derive(Deserialize, Serialize, Type, Clone)]
pub struct ConnectArgs {
  vpnc_script: Option<String>,
  host_identity: Option<HostIdentity>,
  ignore_tls_errors: bool,

  user_agent: Option<String>,
  os: Option<ClientOs>,
  os_version: Option<String>,
  client_version: Option<String>,
  host_id: Option<String>,

  certificate: Option<String>,
  sslkey: Option<String>,
  certificate_data: Option<String>,
  sslkey_data: Option<String>,
  key_password: Option<String>,

  hip_source: HipSource,

  reconnect_timeout: u32,
  mtu: u32,
  disable_ipv6: bool,
  no_dtls: bool,
  local_hostname: Option<String>,
  force_dpd: u32,
  no_xmlpost: bool,
  #[serde(rename = "allowExtendSession")]
  allow_extend_session: bool,
}

impl ConnectArgs {
  fn new() -> Self {
    Self {
      vpnc_script: None,
      host_identity: None,
      ignore_tls_errors: false,
      user_agent: None,
      os: None,
      os_version: None,
      client_version: None,
      host_id: None,
      certificate: None,
      sslkey: None,
      certificate_data: None,
      sslkey_data: None,
      key_password: None,
      hip_source: HipSource::Disabled,
      reconnect_timeout: 300,
      mtu: 0,
      disable_ipv6: false,
      no_dtls: false,
      local_hostname: None,
      force_dpd: 0,
      no_xmlpost: false,
      allow_extend_session: false,
    }
  }

  pub fn host_identity(&self) -> Option<&HostIdentity> {
    self.host_identity.as_ref()
  }

  pub fn ignore_tls_errors(&self) -> bool {
    self.ignore_tls_errors
  }

  pub fn vpnc_script(&self) -> Option<String> {
    self.vpnc_script.clone()
  }

  pub fn user_agent(&self) -> Option<String> {
    self.user_agent.clone()
  }

  pub fn openconnect_os(&self) -> Option<String> {
    self.os.as_ref().map(|os| os.to_openconnect_os().to_string())
  }

  pub fn os(&self) -> Option<ClientOs> {
    self.os.clone()
  }

  pub fn os_version(&self) -> Option<String> {
    self.os_version.clone()
  }

  pub fn client_version(&self) -> Option<String> {
    self.client_version.clone()
  }

  pub fn host_id(&self) -> Option<String> {
    self.host_id.clone()
  }

  pub fn certificate(&self) -> Option<String> {
    self.certificate.clone()
  }

  pub fn sslkey(&self) -> Option<String> {
    self.sslkey.clone()
  }

  pub fn certificate_data(&self) -> anyhow::Result<Option<Vec<u8>>> {
    self
      .certificate_data
      .as_deref()
      .map(crate::utils::base64::decode_to_vec)
      .transpose()
  }

  pub fn sslkey_data(&self) -> anyhow::Result<Option<Vec<u8>>> {
    self
      .sslkey_data
      .as_deref()
      .map(crate::utils::base64::decode_to_vec)
      .transpose()
  }

  pub fn key_password(&self) -> Option<String> {
    self.key_password.clone()
  }

  pub fn hip_source(&self) -> &HipSource {
    &self.hip_source
  }

  pub fn reconnect_timeout(&self) -> u32 {
    self.reconnect_timeout
  }

  pub fn mtu(&self) -> u32 {
    self.mtu
  }

  pub fn disable_ipv6(&self) -> bool {
    self.disable_ipv6
  }

  pub fn no_dtls(&self) -> bool {
    self.no_dtls
  }

  pub fn local_hostname(&self) -> Option<String> {
    self.local_hostname.clone()
  }

  pub fn force_dpd(&self) -> u32 {
    self.force_dpd
  }

  pub fn no_xmlpost(&self) -> bool {
    self.no_xmlpost
  }

  pub fn allow_extend_session(&self) -> bool {
    self.allow_extend_session
  }
}

impl fmt::Debug for ConnectArgs {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ConnectArgs")
      .field("vpnc_script", &self.vpnc_script)
      .field(
        "client_auth_path",
        &(self.certificate.is_some() || self.sslkey.is_some()),
      )
      .field(
        "client_auth_data",
        &(self.certificate_data.is_some() || self.sslkey_data.is_some()),
      )
      .field("hip_source", &self.hip_source)
      .field("reconnect_timeout", &self.reconnect_timeout)
      .field("mtu", &self.mtu)
      .field("disable_ipv6", &self.disable_ipv6)
      .field("no_dtls", &self.no_dtls)
      .field("allow_extend_session", &self.allow_extend_session)
      .finish_non_exhaustive()
  }
}

impl Drop for ConnectArgs {
  fn drop(&mut self) {
    self.key_password.zeroize();
    self.certificate_data.zeroize();
    self.sslkey_data.zeroize();
  }
}

#[derive(Debug, Deserialize, Serialize, Type, Clone)]
pub struct ConnectRequest {
  info: ConnectInfo,
  plan: ConnectionPlan,
  args: ConnectArgs,
}

impl ConnectRequest {
  pub fn new(info: ConnectInfo, plan: ConnectionPlan) -> Self {
    Self {
      info,
      plan,
      args: ConnectArgs::new(),
    }
  }

  pub fn with_vpnc_script<T: Into<Option<String>>>(mut self, vpnc_script: T) -> Self {
    self.args.vpnc_script = vpnc_script.into();
    self
  }

  pub fn with_hip_source(mut self, hip_source: HipSource) -> Self {
    self.args.hip_source = hip_source;
    self
  }

  pub fn with_os_profile(mut self, profile: &OsProfile) -> Self {
    self.args.host_identity = Some(profile.host_identity().clone());
    self.args.os = Some(profile.client_os());
    self.args.os_version = Some(profile.os_version().to_string());
    self.args.client_version = Some(profile.client_version().to_string());
    self.args.host_id = Some(profile.host_identity().host_id().to_string());
    self.args.user_agent = Some(profile.user_agent().to_string());
    self
  }

  pub fn with_ignore_tls_errors(mut self, ignore_tls_errors: bool) -> Self {
    self.args.ignore_tls_errors = ignore_tls_errors;
    self
  }

  pub fn with_certificate<T: Into<Option<String>>>(mut self, certificate: T) -> Self {
    self.args.certificate = certificate.into();
    self
  }

  pub fn with_sslkey<T: Into<Option<String>>>(mut self, sslkey: T) -> Self {
    self.args.sslkey = sslkey.into();
    self
  }

  pub fn with_certificate_data(mut self, certificate: Option<Vec<u8>>) -> Self {
    self.args.certificate_data = certificate.map(encode_secret);
    self
  }

  pub fn with_sslkey_data(mut self, sslkey: Option<Vec<u8>>) -> Self {
    self.args.sslkey_data = sslkey.map(encode_secret);
    self
  }

  pub fn with_key_password<T: Into<Option<String>>>(mut self, key_password: T) -> Self {
    self.args.key_password = key_password.into();
    self
  }

  pub fn with_reconnect_timeout(mut self, reconnect_timeout: u32) -> Self {
    self.args.reconnect_timeout = reconnect_timeout;
    self
  }

  pub fn with_mtu(mut self, mtu: u32) -> Self {
    self.args.mtu = mtu;
    self
  }

  pub fn with_disable_ipv6(mut self, disable_ipv6: bool) -> Self {
    self.args.disable_ipv6 = disable_ipv6;
    self
  }

  pub fn with_no_dtls(mut self, no_dtls: bool) -> Self {
    self.args.no_dtls = no_dtls;
    self
  }

  pub fn with_local_hostname<T: Into<Option<String>>>(mut self, local_hostname: T) -> Self {
    self.args.local_hostname = local_hostname.into();
    self
  }

  pub fn with_force_dpd(mut self, force_dpd: u32) -> Self {
    self.args.force_dpd = force_dpd;
    self
  }

  pub fn with_no_xmlpost(mut self, no_xmlpost: bool) -> Self {
    self.args.no_xmlpost = no_xmlpost;
    self
  }

  pub fn with_allow_extend_session(mut self, allow_extend_session: bool) -> Self {
    self.args.allow_extend_session = allow_extend_session;
    self
  }

  pub fn plan(&self) -> &ConnectionPlan {
    &self.plan
  }

  pub fn info(&self) -> &ConnectInfo {
    &self.info
  }

  pub fn args(&self) -> &ConnectArgs {
    &self.args
  }
}

fn encode_secret(mut data: Vec<u8>) -> String {
  let encoded = crate::utils::base64::encode(&data);
  data.zeroize();
  encoded
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct DisconnectRequest;

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateLogLevelRequest(pub String);

/// Requests that can be sent to the service
#[derive(Debug, Deserialize, Serialize)]
pub enum WsRequest {
  Connect(Box<ConnectRequest>),
  Disconnect(DisconnectRequest),
  StopNonTunnelAttachment { connection_id: uuid::Uuid },
  StoreEditedHipReportChunk(StoreEditedHipReportChunkRequest),
  PreviewHipReport(PreviewHipReportRequest),
  GetHipApprovalStatus { approval_id: String },
  ReadHipPreviewChunk(ReadHipPreviewChunkRequest),
  GetLastSubmittedHipReport,
  UpdateLogLevel(UpdateLogLevelRequest),
  RestartGui,
  UpdateGui(UpdateGuiRequest),
  GetVpncScriptMetadata,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateGuiRequest {
  pub path: String,
  pub checksum: String,
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;
  use crate::os_profile::OsProfileBuilder;
  use crate::{
    gateway::Gateway,
    session::{AuthenticatedGateway, GatewayAuthentication, InternalSessionPolicy},
  };

  fn test_request(info: ConnectInfo) -> ConnectRequest {
    let plan = ConnectionPlan::new(
      vec![AuthenticatedGateway {
        gateway: info.gateway().clone(),
        binding: Some(crate::session::network::test_binding()),
        authentication: GatewayAuthentication::new("authcookie=secret-cookie&user=user".into(), "tunnel".into())
          .unwrap(),
      }],
      vec![],
      InternalSessionPolicy::default(),
    )
    .unwrap();
    ConnectRequest::new(info, plan)
  }

  fn test_connect_info() -> ConnectInfo {
    let gateway = Gateway::new("Gateway".to_string(), "vpn.example.com".to_string());
    ConnectInfo::new("portal.example.com".to_string(), gateway.clone(), vec![gateway])
  }

  #[test]
  fn connect_request_round_trips_a_validated_plan_and_rejects_cookie_only_wire_input() {
    let request = test_request(test_connect_info());
    let mut value = serde_json::to_value(&request).unwrap();
    assert!(value["args"].get("cookie").is_none());
    let decoded: ConnectRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.plan().members().len(), 1);
    assert_eq!(decoded.plan().members()[0].authentication.connection_type(), "tunnel");
    value["plan"]["members"] = json!([]);
    assert!(serde_json::from_value::<ConnectRequest>(value).is_err());
    let mut legacy = serde_json::to_value(&request).unwrap();
    legacy.as_object_mut().unwrap().remove("plan");
    legacy["args"]["cookie"] = json!("authcookie=secret-cookie&user=user");
    assert!(serde_json::from_value::<ConnectRequest>(legacy).is_err());
  }

  #[test]
  fn connect_request_serializes_allow_extend_session() {
    let gateway = Gateway::new("Gateway".to_string(), "vpn.example.com".to_string());
    let info = ConnectInfo::new("portal.example.com".to_string(), gateway.clone(), vec![gateway]);
    let req = test_request(info).with_allow_extend_session(true);
    let value = serde_json::to_value(req).unwrap();

    assert_eq!(value["args"]["allowExtendSession"], json!(true));
  }

  #[test]
  fn with_os_profile_sets_user_agent_from_profile() {
    let profile = OsProfileBuilder::new(ClientOs::Linux).client_version("6.0.0").build();

    let req = test_request(test_connect_info()).with_os_profile(&profile);

    assert_eq!(req.args().user_agent(), Some(profile.user_agent().to_string()));
  }

  #[test]
  fn with_os_profile_sets_host_id_from_profile_runtime_identity() {
    let profile = OsProfileBuilder::new(ClientOs::Linux).build();

    let req = test_request(test_connect_info()).with_os_profile(&profile);

    assert_eq!(
      req.args().host_id(),
      Some(profile.host_identity().host_id().to_string())
    );
  }

  #[test]
  fn client_identity_data_round_trips_without_debug_disclosure() {
    let req = test_request(test_connect_info())
      .with_certificate_data(Some(b"certificate".to_vec()))
      .with_sslkey_data(Some(b"private-key".to_vec()));
    let encoded = serde_json::to_vec(&req).unwrap();
    let decoded: ConnectRequest = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded.args().certificate_data().unwrap().unwrap(), b"certificate");
    assert_eq!(decoded.args().sslkey_data().unwrap().unwrap(), b"private-key");
    let debug = format!("{decoded:?}");
    assert!(!debug.contains("secret-cookie"));
    assert!(!debug.contains("private-key"));
  }
}
