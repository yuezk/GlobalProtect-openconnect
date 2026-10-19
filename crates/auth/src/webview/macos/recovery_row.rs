use std::{
  cell::{Cell, RefCell},
  ffi::c_void,
  ptr,
  sync::Arc,
};

use anyhow::Context;
use block2::RcBlock;
use objc2::{
  DefinedClass, MainThreadOnly, Message, define_class,
  ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject},
  msg_send,
  rc::{Retained, Weak},
  runtime::AnyObject,
  sel,
};
use objc2_app_kit::{
  NSAutoresizingMaskOptions, NSBezelStyle, NSBox, NSBoxType, NSButton, NSColor, NSControlSize, NSEvent, NSEventMask,
  NSEventModifierFlags, NSFont, NSLineBreakMode, NSProgressIndicator, NSProgressIndicatorStyle, NSTextField,
  NSTitlePosition, NSView,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::WKWebView;

use super::super::{
  auth_messenger::{AuthEvent, AuthMessenger},
  recovery::RecoveryStatus,
};

static RECOVERY_ROW_KEY: u8 = 0;

struct RecoveryRowIvars {
  webview: Retained<WKWebView>,
  messenger: Arc<AuthMessenger>,
  row: Retained<NSBox>,
  blocker: Retained<NSView>,
  label: Retained<NSTextField>,
  detail: Retained<NSTextField>,
  proceed: Retained<NSButton>,
  status: Cell<RecoveryStatus>,
  spinner: Retained<NSProgressIndicator>,
  retry: Retained<NSButton>,
  cancel: Retained<NSButton>,
  keyboard_monitor: RefCell<Option<Retained<AnyObject>>>,
}

define_class!(
  #[unsafe(super = NSObject)]
  #[thread_kind = MainThreadOnly]
  #[ivars = RecoveryRowIvars]
  struct RecoveryRow;

  unsafe impl NSObjectProtocol for RecoveryRow {}

  impl RecoveryRow {
    #[unsafe(method(retry:))]
    fn retry(&self, _sender: &AnyObject) {
      self.ivars().messenger.send_auth_event(AuthEvent::Retry);
    }

    #[unsafe(method(continueTls:))]
    fn continue_tls(&self, _sender: &AnyObject) {
      self.ivars().messenger.send_auth_event(AuthEvent::RequestTlsException);
    }

    #[unsafe(method(cancel:))]
    fn cancel(&self, _sender: &AnyObject) {
      self.ivars().messenger.send_auth_event(AuthEvent::Close);
    }
  }
);

impl Drop for RecoveryRow {
  fn drop(&mut self) {
    if let Some(monitor) = self.ivars().keyboard_monitor.borrow_mut().take() {
      unsafe {
        NSEvent::removeMonitor(&monitor);
      }
    }
  }
}

impl RecoveryRow {
  fn handle_key(&self, event: &NSEvent) -> bool {
    let controls = self.ivars();
    if !controls.status.get().blocks_page() {
      return false;
    }
    let Some(window) = controls.webview.window() else {
      return false;
    };
    if window.attachedSheet().is_some() || event.window(self.mtm()).as_deref() != Some(&*window) {
      return false;
    }
    let responder = window.firstResponder();
    let modifiers = event.modifierFlags();
    if event.keyCode() == 48 && !modifiers.intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control) {
      // Explicit traversal also works when the system skips buttons in its default key loop.
      let Some(address) = super::address_bar::address_view(&controls.webview) else {
        return false;
      };
      let mut views: Vec<&NSView> = vec![&address];
      if !controls.retry.isHidden() {
        views.push(&controls.retry);
      }
      if !controls.proceed.isHidden() {
        views.push(&controls.proceed);
      }
      views.push(&controls.cancel);
      let current = responder.as_ref().and_then(|responder| {
        views.iter().position(|view| {
          ptr::eq(
            (*view) as *const NSView as *const c_void,
            Retained::as_ptr(responder).cast(),
          )
        })
      });
      let next = match current {
        Some(index) if modifiers.contains(NSEventModifierFlags::Shift) => (index + views.len() - 1) % views.len(),
        Some(index) => (index + 1) % views.len(),
        None => views.len() - 1,
      };
      window.makeFirstResponder(Some(views[next]));
      return true;
    }
    // If WebKit regains focus programmatically, do not deliver keys to the covered page.
    if responder
      .as_deref()
      .and_then(|responder| responder.downcast_ref::<NSView>())
      .is_some_and(|view| view.isDescendantOf(&controls.webview))
    {
      window.makeFirstResponder(Some(&controls.cancel));
      return true;
    }
    false
  }
}

pub(super) fn install(webview: &WKWebView, messenger: Arc<AuthMessenger>) -> anyhow::Result<()> {
  let mtm = MainThreadMarker::new().context("Recovery controls require the main thread")?;
  let parent = unsafe { webview.superview() }.context("Webview has no containing view")?;
  let row = NSBox::new(mtm);
  // AppKit may resize the parent after the application's window resize callback.
  row.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin);
  row.setBoxType(NSBoxType::Custom);
  row.setTitlePosition(NSTitlePosition::NoTitle);
  row.setContentViewMargins(NSSize::new(0.0, 0.0));
  row.setBorderWidth(0.0);
  row.setCornerRadius(0.0);
  row.setFillColor(&NSColor::windowBackgroundColor());
  let label = NSTextField::labelWithString(&NSString::new(), mtm);
  label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
  label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
  label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
  let detail = NSTextField::labelWithString(&NSString::new(), mtm);
  detail.setFont(Some(&NSFont::systemFontOfSize(11.0)));
  detail.setTextColor(Some(&NSColor::secondaryLabelColor()));
  detail.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
  detail.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
  let spinner = NSProgressIndicator::new(mtm);
  spinner.setStyle(NSProgressIndicatorStyle::Spinning);
  spinner.setControlSize(NSControlSize::Small);
  spinner.setIndeterminate(true);
  let retry = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str("Try Again"), None, None, mtm) };
  let cancel = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str("Cancel"), None, None, mtm) };
  let proceed =
    unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str("Continue Anyway…"), None, None, mtm) };
  for button in [&retry, &proceed, &cancel] {
    button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
    button.setBezelStyle(NSBezelStyle::Push);
    button.setControlSize(NSControlSize::Small);
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    row.addSubview(button);
  }
  row.addSubview(&label);
  row.addSubview(&detail);
  row.addSubview(&spinner);
  let blocker = NSView::new(mtm);
  blocker
    .setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
  parent.addSubview(&blocker);
  parent.addSubview(&row);
  row.setHidden(true);
  blocker.setHidden(true);
  let controller = RecoveryRow::alloc(mtm).set_ivars(RecoveryRowIvars {
    webview: webview.retain(),
    messenger,
    row,
    blocker,
    label,
    detail,
    proceed,
    status: Cell::new(RecoveryStatus::Hidden),
    spinner,
    retry,
    cancel,
    keyboard_monitor: RefCell::new(None),
  });
  let controller: Retained<RecoveryRow> = unsafe { msg_send![super(controller), init] };
  let weak_controller = Weak::from_retained(&controller);
  let handler = RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| {
    let blocked = weak_controller
      .load()
      .is_some_and(|controller| controller.handle_key(unsafe { event.as_ref() }));
    if blocked { ptr::null_mut() } else { event.as_ptr() }
  });
  let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler) }
    .context("Failed to guard keyboard input during sign-in recovery")?;
  *controller.ivars().keyboard_monitor.borrow_mut() = Some(monitor);
  unsafe {
    controller.ivars().retry.setTarget(Some(&controller));
    controller.ivars().retry.setAction(Some(sel!(retry:)));
    controller.ivars().proceed.setTarget(Some(&controller));
    controller.ivars().proceed.setAction(Some(sel!(continueTls:)));
    controller.ivars().cancel.setTarget(Some(&controller));
    controller.ivars().cancel.setAction(Some(sel!(cancel:)));
    objc_setAssociatedObject(
      Retained::as_ptr(&parent).cast_mut().cast(),
      &RECOVERY_ROW_KEY as *const u8 as *const c_void,
      Retained::as_ptr(&controller).cast_mut().cast(),
      OBJC_ASSOCIATION_RETAIN_NONATOMIC,
    );
  }
  Ok(())
}

fn controller(webview: &WKWebView) -> Option<Retained<RecoveryRow>> {
  let parent = unsafe { webview.superview() }?;
  unsafe {
    let pointer = objc_getAssociatedObject(
      Retained::as_ptr(&parent).cast_mut().cast(),
      &RECOVERY_ROW_KEY as *const u8 as *const c_void,
    )
    .cast::<RecoveryRow>();
    Retained::retain(pointer.cast_mut())
  }
}

pub(super) fn update(webview: &WKWebView, status: RecoveryStatus) -> anyhow::Result<()> {
  let controller = controller(webview).context("Recovery controls are unavailable")?;
  let controls = controller.ivars();
  let was_blocking = controls.status.replace(status).blocks_page();
  let hidden = status == RecoveryStatus::Hidden;
  controls.label.setStringValue(&NSString::from_str(status.message()));
  controls.detail.setStringValue(&NSString::from_str(status.detail()));
  controls.detail.setHidden(status.detail().is_empty());
  controls.retry.setHidden(!status.can_retry());
  controls.proceed.setHidden(!status.can_continue());
  controls.cancel.setHidden(!status.blocks_page());
  controls.spinner.setHidden(!status.is_busy());
  unsafe {
    if status.is_busy() {
      controls.spinner.startAnimation(None);
    } else {
      controls.spinner.stopAnimation(None);
    }
  }
  controls.row.setHidden(hidden);
  controls.blocker.setHidden(!status.blocks_page());
  if was_blocking != status.blocks_page() {
    if let Some(window) = webview.window() {
      if !status.blocks_page() {
        window.makeFirstResponder(Some(webview));
      } else {
        window.makeFirstResponder(Some(&controls.cancel));
      }
    }
  }
  super::address_bar::resize(webview);
  Ok(())
}

/// Position the row above the page and return the height reserved for it.
pub(super) fn layout(webview: &WKWebView, content: NSRect) -> f64 {
  let Some(controller) = controller(webview) else {
    return 0.0;
  };
  let controls = controller.ivars();
  if controls.row.isHidden() {
    return 0.0;
  }
  let height = if controls.status.get().detail().is_empty() {
    36.0
  } else {
    52.0
  };
  let width = content.size.width;
  controls.row.setFrame(NSRect::new(
    NSPoint::new(content.origin.x, content.origin.y + content.size.height - height),
    NSSize::new(width, height),
  ));
  controls.blocker.setFrame(NSRect::new(
    content.origin,
    NSSize::new(width, (content.size.height - height).max(0.0)),
  ));
  controls.cancel.setFrame(NSRect::new(
    NSPoint::new(width - 80.0, (height - 26.0) / 2.0),
    NSSize::new(72.0, 26.0),
  ));
  controls.retry.setFrame(NSRect::new(
    NSPoint::new(
      width - 162.0 - if controls.proceed.isHidden() { 0.0 } else { 132.0 },
      (height - 26.0) / 2.0,
    ),
    NSSize::new(78.0, 26.0),
  ));
  controls.proceed.setFrame(NSRect::new(
    NSPoint::new(width - 212.0, (height - 26.0) / 2.0),
    NSSize::new(128.0, 26.0),
  ));
  controls
    .spinner
    .setFrame(NSRect::new(NSPoint::new(12.0, 10.0), NSSize::new(16.0, 16.0)));
  let left = if controls.spinner.isHidden() { 12.0 } else { 36.0 };
  let right = if !controls.proceed.isHidden() {
    width - 220.0
  } else if !controls.retry.isHidden() {
    width - 170.0
  } else {
    width - 88.0
  };
  controls.label.setFrame(NSRect::new(
    NSPoint::new(left, height - 26.0),
    NSSize::new((right - left).max(0.0), 17.0),
  ));
  controls.detail.setFrame(NSRect::new(
    NSPoint::new(left, 7.0),
    NSSize::new((right - left).max(0.0), 15.0),
  ));
  height
}
