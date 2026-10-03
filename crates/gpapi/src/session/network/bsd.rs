pub(super) use super::bsd_routes::routes;
use super::*;

pub(super) fn resolvers(control: &dyn CollectionControl) -> anyhow::Result<Vec<ResolverContext>> {
  resolv_conf(control)
}
