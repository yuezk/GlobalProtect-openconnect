use roxmltree::Document;

pub(crate) fn get_child_text(doc: &Document, name: &str) -> Option<String> {
  let node = doc.descendants().find(|n| n.has_tag_name(name))?;
  node.text().map(|s| s.to_string())
}
