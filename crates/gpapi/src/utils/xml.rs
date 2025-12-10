use xmltree::Element;

pub(crate) trait ElementExt {
  /// Recursively find all descendants with the given name
  fn descendants(&self, name: &str) -> Vec<&Element>;

  /// Recursively find the first descendant with the given name
  fn descendant(&self, name: &str) -> Option<&Element>;

  /// Recursively find the text of the first descendant with the given name
  fn descendant_text(&self, name: &str) -> Option<String>;

  /// Find the first child with the given name
  fn child(&self, name: &str) -> Option<&Element>;

  /// Find the text of the first child with the given name
  fn child_text(&self, name: &str) -> Option<String>;

  /// Get the direct child element with the given name
  fn children(&self, name: &str) -> Vec<&Element>;

  /// Get the attribute value by name
  fn attr(&self, name: &str) -> Option<&str>;
}

impl ElementExt for Element {
  fn descendants(&self, name: &str) -> Vec<&Element> {
    let mut results = Vec::new();

    if self.name == name {
      results.push(self);
    }

    for child in &self.children {
      if let Some(element) = child.as_element() {
        results.extend(element.descendants(name));
      }
    }

    results
  }

  fn descendant(&self, name: &str) -> Option<&Element> {
    if self.name == name {
      return Some(self);
    }

    for child in &self.children {
      if let Some(element) = child.as_element() {
        if let Some(found) = element.descendant(name) {
          return Some(found);
        }
      }
    }

    None
  }

  fn descendant_text(&self, name: &str) -> Option<String> {
    self
      .descendant(name)
      .and_then(|element| element.get_text().map(|s| s.to_string()))
  }

  fn child(&self, name: &str) -> Option<&Element> {
    self.get_child(name)
  }

  fn child_text(&self, name: &str) -> Option<String> {
    self
      .get_child(name)
      .and_then(|element| element.get_text().map(|s| s.to_string()))
  }

  fn children(&self, name: &str) -> Vec<&Element> {
    self
      .children
      .iter()
      .filter_map(|child| child.as_element())
      .filter(|element| element.name == name)
      .collect()
  }

  fn attr(&self, name: &str) -> Option<&str> {
    self.attributes.get(name).map(|s| s.as_str())
  }
}
