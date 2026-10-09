use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ScriptSource<'a> {
  Installed(&'a Path),
  Legacy(&'a str),
  Default,
}

pub(super) fn select<'a>(installed: Option<&'a Path>, legacy: Option<&'a str>) -> ScriptSource<'a> {
  if let Some(path) = installed {
    return ScriptSource::Installed(path);
  }
  if let Some(script) = legacy {
    return ScriptSource::Legacy(script);
  }
  ScriptSource::Default
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn installed_script_takes_precedence_over_legacy() {
    let installed = Path::new("/var/lib/gpclient/scripts/vpnc-script");
    assert_eq!(
      select(Some(installed), Some("legacy-command")),
      ScriptSource::Installed(installed)
    );
  }

  #[test]
  fn legacy_script_is_used_before_default() {
    assert_eq!(
      select(None, Some("legacy-command")),
      ScriptSource::Legacy("legacy-command")
    );
    assert_eq!(select(None, None), ScriptSource::Default);
  }
}
