use super::*;
use crate::{
  gateway::{GatewayLogin, GatewayLoginClient},
  gp_params::GpParams,
  os_profile::{ClientOs, OsProfile},
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn failed_binding_retains_issued_session_for_exactly_once_logout() {
  use crate::session::transport::tests::{login_gateway, response};
  let mut arguments = [""; 19];
  arguments[1] = "issued-cookie";
  arguments[4] = "alice";
  arguments[12] = "non-tunnel";
  let xml = format!(
    "<jnlp><application-desc>{}</application-desc></jnlp>",
    arguments
      .iter()
      .map(|value| format!("<argument>{value}</argument>"))
      .collect::<String>()
  );
  let (_, origin, server) = login_gateway(vec![response(&xml), response("<response status='success'/>")]).await;
  let params = GpParams::builder(
    OsProfile::builder(ClientOs::Linux)
      .computer_name_override("original-computer")
      .build(crate::os_profile::test_snapshot()),
  )
  .build();
  let mut client = GatewayLoginClient::new(origin.as_str(), params).unwrap();
  let credential = crate::credential::PasswordCredential::new("alice", "password").into();
  let GatewayLogin::Authenticated(authentication) = client
    .login(&credential, None, &CancellationToken::new())
    .await
    .unwrap()
  else {
    panic!("expected issued authentication")
  };
  let gateway = Gateway::new(origin.to_string(), origin.to_string());
  let mut sessions = GatewaySessions::default();
  sessions.register_login(gateway.clone(), authentication, &client);
  // HTTP is accepted by the protocol fixture but rejected by native binding validation.
  assert!(
    sessions
      .bind_non_tunnel(&gateway, &client, false, None, &CancellationToken::new())
      .await
      .is_err()
  );
  assert_eq!(sessions.members().len(), 1);
  assert!(
    sessions.members()[0]
      .transport
      .authentication()
      .cookie()
      .contains("issued-cookie")
  );
  sessions.logout_gateway(gateway.server()).await;
  assert!(sessions.is_empty());
  sessions.logout().await;
  let requests = server.await.unwrap();
  assert_eq!(requests.len(), 2);
  assert_eq!(requests[1].0, "/ssl-vpn/logout.esp");
  assert!(requests[1].1.contains("authcookie=issued-cookie"));
  assert!(requests[1].1.contains("computer=original-computer"));
}
