use std::future::Future;

use gpapi::session::GatewaySessionSummary;

use super::ConnectHandler;

pub(super) struct InternalOutput;

impl InternalOutput {
  pub(super) fn prepare(
    _: &ConnectHandler<'_>,
    _: &str,
  ) -> anyhow::Result<(Self, impl Fn(&GatewaySessionSummary) + Send + Sync + 'static + use<>)> {
    Ok((Self, |_: &GatewaySessionSummary| {}))
  }

  pub(super) async fn run<T>(self, maintenance: impl Future<Output = T>) -> (T, anyhow::Result<()>) {
    (maintenance.await, Ok(()))
  }
}
