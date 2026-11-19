use std::{future::Future, sync::Mutex};

use gpapi::session::{GatewaySessionSummary, MaintenanceState};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::dashboard::{Dashboard, InternalMembers};

use super::ConnectHandler;

pub(super) struct InternalOutput(Option<DashboardState>);

struct DashboardState {
  dashboard: Dashboard,
  members: InternalMembers,
  established: oneshot::Receiver<()>,
  cancellation: CancellationToken,
}

impl InternalOutput {
  pub(super) fn prepare(
    handler: &ConnectHandler<'_>,
    gateway: &str,
  ) -> anyhow::Result<(Self, impl Fn(&GatewaySessionSummary) + Send + Sync + 'static + use<>)> {
    let (sender, established) = oneshot::channel();
    let established_sender = Mutex::new(Some(sender));
    let members = InternalMembers::default();
    let dashboard_members = members.clone();
    let output = if handler.args.dashboard {
      Some(DashboardState {
        dashboard: Dashboard::prepare(gateway.to_owned(), handler.cancellation.clone())?,
        members,
        established,
        cancellation: handler.cancellation.clone(),
      })
    } else {
      None
    };
    let observe = move |member: &GatewaySessionSummary| {
      if member.maintenance == MaintenanceState::Healthy
        && let Some(ready) = established_sender.lock().unwrap().take()
      {
        let _ = ready.send(());
      }
      dashboard_members.lock().unwrap().insert(
        member.gateway.server().to_owned(),
        format!("{}: {:?}", member.gateway, member.maintenance),
      );
    };
    Ok((Self(output), observe))
  }

  pub(super) async fn run<T>(self, maintenance: impl Future<Output = T>) -> (T, anyhow::Result<()>) {
    let Some(mut output) = self.0 else {
      return (maintenance.await, Ok(()));
    };
    let mut start = Ok(());
    tokio::pin!(maintenance);
    let result = tokio::select! {
      result = &mut maintenance => result,
      established = &mut output.established => {
        if established.is_ok() {
          start = output.dashboard.start_internal(output.members);
          if start.is_err() {
            output.cancellation.cancel();
          }
        }
        maintenance.await
      }
    };
    (result, output.dashboard.finish().and(start))
  }
}
