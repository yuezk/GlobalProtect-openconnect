use std::{ffi::c_void, ptr};

use anyhow::Context;
use objc2::{
  DefinedClass, MainThreadOnly, Message, define_class,
  ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject},
  msg_send,
  rc::Retained,
  runtime::{AnyObject, ProtocolObject},
};
use objc2_app_kit::{
  NSAccessibility, NSAutoresizingMaskOptions, NSBox, NSBoxType, NSColor, NSFont, NSLayoutConstraint,
  NSLayoutConstraintOrientation, NSLineBreakMode, NSTextView, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode,
  NSToolbarItem, NSToolbarItemIdentifier, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_foundation::{
  MainThreadMarker, NSArray, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions, NSObject,
  NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use objc2_web_kit::WKWebView;

use super::super::address_bar::{LOADING_ADDRESS, display_address};

static ADDRESS_TOOLBAR_KEY: u8 = 0;
const ADDRESS_ITEM_ID: &str = "com.yuezk.gpauth.address";

define_class!(
  #[unsafe(super = NSTextView)]
  #[thread_kind = MainThreadOnly]
  struct AddressView;

  impl AddressView {
    #[unsafe(method(intrinsicContentSize))]
    fn intrinsic_content_size(&self) -> NSSize {
      // URL length must not determine the toolbar item's width.
      NSSize::new(-1.0, 22.0)
    }
  }
);

struct AddressToolbarIvars {
  webview: Retained<WKWebView>,
  address_view: Retained<NSTextView>,
  separator: Retained<NSBox>,
}

define_class!(
  // The content view retains the delegate because NSToolbar's delegate is weak.
  // Drop unregisters URL observation before releasing the retained webview.
  #[unsafe(super = NSObject)]
  #[thread_kind = MainThreadOnly]
  #[ivars = AddressToolbarIvars]
  struct AddressToolbar;

  unsafe impl NSObjectProtocol for AddressToolbar {}

  unsafe impl NSToolbarDelegate for AddressToolbar {
    #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
    fn toolbar_item(
      &self,
      _toolbar: &NSToolbar,
      identifier: &NSToolbarItemIdentifier,
      inserting: bool,
    ) -> Option<Retained<NSToolbarItem>> {
      if !inserting || !identifier.isEqualToString(&NSString::from_str(ADDRESS_ITEM_ID)) {
        None
      } else {
        let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(self.mtm()), identifier);
        item.setLabel(&NSString::from_str("Sign-in page address"));
        item.setBordered(true);
        item.setView(Some(&self.ivars().address_view));
        Some(item)
      }
    }

    #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
    fn default_items(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
      NSArray::from_retained_slice(&[NSString::from_str(ADDRESS_ITEM_ID)])
    }

    #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
    fn allowed_items(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
      NSArray::from_retained_slice(&[NSString::from_str(ADDRESS_ITEM_ID)])
    }
  }

  impl AddressToolbar {
    #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
    fn observe_url(
      &self,
      _key_path: Option<&NSString>,
      _object: Option<&AnyObject>,
      _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
      _context: *mut c_void,
    ) {
      let uri = unsafe { self.ivars().webview.URL() }.and_then(|url| url.absoluteString());
      let address = display_address(uri.as_ref().map(|uri| uri.to_string()).as_deref());
      self.ivars().address_view.setString(&NSString::from_str(&address));
    }
  }
);

impl Drop for AddressToolbar {
  fn drop(&mut self) {
    // This private observer registers only URL and remains on the main thread.
    unsafe {
      self
        .ivars()
        .webview
        .removeObserver_forKeyPath(self, &NSString::from_str("URL"));
    }
  }
}

pub(super) fn install(webview: &WKWebView) -> anyhow::Result<()> {
  let mtm = MainThreadMarker::new().context("Toolbar must be installed on the main thread")?;
  let parent = unsafe { webview.superview() }.context("Webview has no containing view")?;
  let window = webview.window().context("Webview has no window")?;
  parent.setWantsLayer(true);
  let address_view: Retained<AddressView> =
    unsafe { msg_send![AddressView::alloc(mtm), initWithFrame: NSRect::default()] };
  // Use one native text layout for display and selection.
  address_view.setString(&NSString::from_str(LOADING_ADDRESS));
  address_view.setDrawsBackground(false);
  // Keep NSTextView's visible-rect cursor tracking inside the address bar.
  address_view.setClipsToBounds(true);
  address_view.setRichText(false);
  address_view.setHorizontallyResizable(false);
  address_view.setVerticallyResizable(false);
  address_view.setTextContainerInset(NSSize::new(6.0, 3.0));
  address_view.setEditable(false);
  address_view.setSelectable(true);
  address_view.setFont(Some(&NSFont::systemFontOfSize(12.0)));
  address_view.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
  address_view.setAccessibilityLabel(Some(&NSString::from_str("Sign-in page address")));
  if let Some(container) = unsafe { address_view.textContainer() } {
    container.setWidthTracksTextView(true);
    container.setLineFragmentPadding(0.0);
    container.setMaximumNumberOfLines(1);
    container.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
  }
  // AppKit measures the toolbar item's minimum and maximum width from its view.
  NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
    address_view.widthAnchor().constraintGreaterThanOrEqualToConstant(200.0),
    address_view.widthAnchor().constraintLessThanOrEqualToConstant(10000.0),
    address_view.heightAnchor().constraintEqualToConstant(22.0),
  ]));
  let separator = NSBox::new(mtm);
  separator
    .setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin);
  separator.setBoxType(NSBoxType::Custom);
  separator.setBorderWidth(0.0);
  separator.setCornerRadius(0.0);
  separator.setFillColor(&NSColor::separatorColor());
  parent.addSubview(&separator);
  let delegate = AddressToolbar::alloc(mtm).set_ivars(AddressToolbarIvars {
    webview: webview.retain(),
    address_view: Retained::into_super(address_view),
    separator,
  });
  let delegate: Retained<AddressToolbar> = unsafe { msg_send![super(delegate), init] };
  // Observe the current URL independently of navigation callbacks.
  unsafe {
    webview.addObserver_forKeyPath_options_context(
      &delegate,
      &NSString::from_str("URL"),
      NSKeyValueObservingOptions::Initial,
      ptr::null_mut(),
    );
    objc_setAssociatedObject(
      Retained::as_ptr(&parent).cast_mut().cast(),
      &ADDRESS_TOOLBAR_KEY as *const u8 as *const c_void,
      Retained::as_ptr(&delegate).cast_mut().cast(),
      OBJC_ASSOCIATION_RETAIN_NONATOMIC,
    );
  }
  let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str("com.yuezk.gpauth.toolbar"));
  toolbar.setAllowsUserCustomization(false);
  toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
  toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
  window.setTitleVisibility(NSWindowTitleVisibility::Visible);
  if objc2::available!(macos = 11.0) {
    window.setToolbarStyle(NSWindowToolbarStyle::Expanded);
  }
  window.setToolbar(Some(&toolbar));
  window.setInitialFirstResponder(Some(webview));
  resize(webview);
  Ok(())
}

pub(super) fn address_view(webview: &WKWebView) -> Option<Retained<NSTextView>> {
  let parent = unsafe { webview.superview() }?;
  let delegate = unsafe {
    objc_getAssociatedObject(
      Retained::as_ptr(&parent).cast_mut().cast(),
      &ADDRESS_TOOLBAR_KEY as *const u8 as *const c_void,
    )
    .cast::<AddressToolbar>()
    .as_ref()
  }?;
  Some(delegate.ivars().address_view.clone())
}

pub(super) fn show_address(webview: &WKWebView, uri: &str) {
  if let Some(view) = address_view(webview) {
    view.setString(&NSString::from_str(&display_address(Some(uri))));
  }
}

pub(super) fn resize(webview: &WKWebView) {
  let Some(parent) = (unsafe { webview.superview() }) else {
    return;
  };
  let Some(window) = webview.window() else {
    return;
  };
  // Keep the native separator outside WebKit so page backgrounds cannot cover it.
  let content = parent.convertRect_fromView(window.contentLayoutRect(), None);
  let separator_height = 1.0 / window.backingScaleFactor();
  let recovery_height = super::recovery_row::layout(webview, content);
  let page_height = (content.size.height - recovery_height - separator_height).max(0.0);
  let delegate = unsafe {
    objc_getAssociatedObject(
      Retained::as_ptr(&parent).cast_mut().cast(),
      &ADDRESS_TOOLBAR_KEY as *const u8 as *const c_void,
    )
    .cast::<AddressToolbar>()
    .as_ref()
  };
  if let Some(delegate) = delegate {
    delegate.ivars().separator.setFrame(NSRect::new(
      NSPoint::new(content.origin.x, content.origin.y + page_height),
      NSSize::new(content.size.width, separator_height),
    ));
  }
  webview.setFrame(NSRect::new(
    content.origin,
    NSSize::new(content.size.width, page_height),
  ));
}
