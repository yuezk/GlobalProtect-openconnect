use crate::error::PortalError;

pub mod args;

pub trait Args {
  fn fix_openssl(&self) -> bool;
  fn ignore_tls_errors(&self) -> bool;
}

pub fn handle_error(err: anyhow::Error, args: &impl Args) {
  eprintln!("\nError: {}", err);

  let Some(err) = err.downcast_ref::<PortalError>() else {
    return;
  };

  if err.is_legacy_openssl_error() && !args.fix_openssl() {
    eprintln!("\nRe-run it with the `--fix-openssl` option to work around this issue, e.g.:\n");
    let args = std::env::args().collect::<Vec<_>>();
    eprintln!("{} --fix-openssl {}\n", args[0], args[1..].join(" "));
  }

  if err.is_tls_error() && !args.ignore_tls_errors() {
    eprintln!("\nRe-run it with the `--ignore-tls-errors` option to ignore the certificate error, e.g.:\n");
    let args = std::env::args().collect::<Vec<_>>();
    eprintln!("{} --ignore-tls-errors {}\n", args[0], args[1..].join(" "));
  }
}
