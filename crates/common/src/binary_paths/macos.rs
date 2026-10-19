use std::path::{Path, PathBuf};

fn contents_dir(executable: &Path) -> Option<&Path> {
  let bin_dir = executable.parent()?;
  let contents = bin_dir.parent()?;
  let bundle = contents.parent()?;
  let directory_name = bin_dir.file_name()?;
  if (directory_name != "MacOS" && directory_name != "Helpers")
    || contents.file_name()? != "Contents"
    || bundle.extension()? != "app"
  {
    return None;
  }
  Some(contents)
}

pub(super) fn helper_path(executable: &Path, binary_name: &str) -> Option<PathBuf> {
  Some(contents_dir(executable)?.join("Helpers").join(binary_name))
}
