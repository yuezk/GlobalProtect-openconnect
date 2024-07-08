use roxmltree::Node;

pub(crate) trait NodeExt<'a> {
  fn find_descendant(&self, name: &str) -> Option<Node<'a, 'a>>;
  fn descendant_text(&self, name: &str) -> Option<&'a str>;

  fn find_child(&self, name: &str) -> Option<Node<'a, 'a>>;
  fn child_text(&self, name: &str) -> Option<&'a str>;
}

impl<'a> NodeExt<'a> for Node<'a, 'a> {
  fn find_descendant(&self, name: &str) -> Option<Node<'a, 'a>> {
    self.descendants().find(|n| n.has_tag_name(name))
  }

  fn descendant_text(&self, name: &str) -> Option<&'a str> {
    self.find_descendant(name).and_then(|node| node.text())
  }

  fn find_child(&self, name: &str) -> Option<Node<'a, 'a>> {
    self.children().find(|n| n.has_tag_name(name))
  }

  fn child_text(&self, name: &str) -> Option<&'a str> {
    self.find_child(name).and_then(|node| node.text())
  }
}
