pub(super) use super::bsd_routes::routes;
use super::*;

pub(super) fn scope_resolver_socket(
  _socket: &tokio::net::UdpSocket,
  _interface: &Interface,
  _endpoint: SocketAddr,
) -> anyhow::Result<()> {
  // resolv.conf has no per-interface resolver contexts. IPv6 zones are carried
  // in the destination address instead.
  anyhow::bail!("Interface-scoped resolver contexts are unsupported on BSD")
}

pub(super) fn resolvers(control: &dyn CollectionControl) -> anyhow::Result<Vec<ResolverContext>> {
  resolv_conf(control)
}
