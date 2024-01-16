use std::path::Path;

use tempfile::NamedTempFile;

pub fn openssl_conf() -> String {
  let option = "UnsafeLegacyServerConnect";

  format!(
    "openssl_conf = openssl_init

[openssl_init]
ssl_conf = ssl_sect

[ssl_sect]
system_default = system_default_sect

[system_default_sect]
Options = {}",
    option
  )
}

pub fn fix_openssl<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
  let content = openssl_conf();
  std::fs::write(path, content)?;
  Ok(())
}

pub fn fix_openssl_env() -> anyhow::Result<NamedTempFile> {
  let openssl_conf = NamedTempFile::new()?;
  let openssl_conf_path = openssl_conf.path();

  fix_openssl(openssl_conf_path)?;
  std::env::set_var("OPENSSL_CONF", openssl_conf_path);

  Ok(openssl_conf)
}
