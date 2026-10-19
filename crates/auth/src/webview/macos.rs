use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  ffi::c_void,
  ptr,
  sync::Arc,
};

use anyhow::{Context, bail};
use block2::{DynBlock, RcBlock};
use gpapi::utils::redact::redact_uri;
use log::{info, warn};
use objc2::{
  DefinedClass, MainThreadOnly, Message, define_class,
  ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject},
  msg_send,
  rc::Retained,
  runtime::{AnyObject, ProtocolObject, Sel},
  sel,
};
use objc2_foundation::{
  MainThreadMarker, NSError, NSObject, NSObjectNSKeyValueCoding, NSObjectProtocol, NSString, NSURL,
  NSURLAuthenticationChallenge, NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLErrorCancelled,
  NSURLErrorDomain, NSURLErrorFailingURLErrorKey, NSURLErrorUnsupportedURL, NSURLRequest,
  NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView};
use tauri::webview::PlatformWebview;

use super::{
  auth_messenger::{AuthError, AuthEvent, AuthMessenger},
  recovery::RecoveryStatus,
  webview_auth::PlatformWebviewExt,
};

#[path = "macos/address_bar.rs"]
mod address_bar;
#[path = "macos/recovery_row.rs"]
mod recovery_row;
#[path = "macos/tls.rs"]
mod tls;

static AUTH_NAVIGATION_DELEGATE_KEY: u8 = 0;

struct AuthNavigationDelegateIvars {
  original: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
  messenger: Arc<AuthMessenger>,
  ignore_tls_errors: Cell<bool>,
  navigations: RefCell<HashMap<usize, (Retained<WKNavigation>, u64)>>,
  tls: RefCell<tls::TlsState>,
}

define_class!(
  #[unsafe(super = NSObject)]
  #[thread_kind = MainThreadOnly]
  #[ivars = AuthNavigationDelegateIvars]
  struct AuthNavigationDelegate;

  unsafe impl NSObjectProtocol for AuthNavigationDelegate {}

  #[allow(non_snake_case)]
  unsafe impl WKNavigationDelegate for AuthNavigationDelegate {
    #[unsafe(method(webView:didStartProvisionalNavigation:))]
    unsafe fn webView_didStartProvisionalNavigation(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
      self.track_navigation(navigation);
      self.ivars().messenger.cancel_raise_window();
      let original = &self.ivars().original;
      if original.respondsToSelector(sel!(webView:didStartProvisionalNavigation:)) {
        unsafe { original.webView_didStartProvisionalNavigation(webview, navigation); }
      }
    }

    #[unsafe(method(webView:didCommitNavigation:))]
    unsafe fn webView_didCommitNavigation(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
      if let Some(generation) = self.navigation_generation(navigation) {
        info!("Started loading page: {}", navigation_address(webview));
        read_navigation_html(webview, Arc::clone(&self.ivars().messenger), generation);
      }
      let original = &self.ivars().original;
      if original.respondsToSelector(sel!(webView:didCommitNavigation:)) {
        unsafe { original.webView_didCommitNavigation(webview, navigation); }
      }
    }

    #[unsafe(method(webView:didFinishNavigation:))]
    unsafe fn webView_didFinishNavigation(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
      if let Some(generation) = self.finish_navigation(navigation) {
        info!("Finished loading page: {}", navigation_address(webview));
        read_navigation_html(webview, Arc::clone(&self.ivars().messenger), generation);
        self.ivars().messenger.send_auth_event(AuthEvent::PageLoaded(generation));
      }
      let original = &self.ivars().original;
      if original.respondsToSelector(sel!(webView:didFinishNavigation:)) {
        unsafe { original.webView_didFinishNavigation(webview, navigation); }
      }
    }

    #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
    unsafe fn webView_didFailProvisionalNavigation_withError(&self, webview: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
      self.report_load_failure(webview, navigation, error);
      let original = &self.ivars().original;
      if original.respondsToSelector(sel!(webView:didFailProvisionalNavigation:withError:)) {
        unsafe { original.webView_didFailProvisionalNavigation_withError(webview, navigation, error); }
      }
    }

    #[unsafe(method(webView:didFailNavigation:withError:))]
    unsafe fn webView_didFailNavigation_withError(&self, webview: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
      self.report_load_failure(webview, navigation, error);
      let original = &self.ivars().original;
      if original.respondsToSelector(sel!(webView:didFailNavigation:withError:)) {
        unsafe { original.webView_didFailNavigation_withError(webview, navigation, error); }
      }
    }

    #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
    unsafe fn webView_didReceiveAuthenticationChallenge_completionHandler(
      &self,
      webview: &WKWebView,
      challenge: &NSURLAuthenticationChallenge,
      completion_handler: &DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
    ) {
      // Preserve Wry's normal authentication challenge handling unless the caller
      // approved this certificate or requested the ignore-TLS-errors behavior.
      let protection_space = challenge.protectionSpace();
      let is_server_trust = protection_space.authenticationMethod().isEqualToString(unsafe { NSURLAuthenticationMethodServerTrust });
      let server_trust: *mut AnyObject = if is_server_trust {
        msg_send![&*protection_space, serverTrust]
      } else { ptr::null_mut() };
      let approved = std::ptr::NonNull::new(server_trust.cast::<objc2_security::SecTrust>()).is_some_and(|trust| {
        self.ivars().tls.borrow().allows(&protection_space.host().to_string(), unsafe { trust.as_ref() })
      });
      if !self.ivars().ignore_tls_errors.get() && !approved {
        let original = &self.ivars().original;
        if original.respondsToSelector(sel!(webView:didReceiveAuthenticationChallenge:completionHandler:)) {
          unsafe { original.webView_didReceiveAuthenticationChallenge_completionHandler(webview, challenge, completion_handler); }
        } else {
          completion_handler.call((challenge_disposition(false), ptr::null_mut()));
        }
        return;
      }
      if !is_server_trust {
        completion_handler.call((challenge_disposition(false), ptr::null_mut()));
        return;
      }

      if server_trust.is_null() {
        completion_handler.call((challenge_disposition(false), ptr::null_mut()));
        return;
      }

      let credential: Retained<NSURLCredential> =
        msg_send![objc2::class!(NSURLCredential), credentialForTrust: server_trust];
      completion_handler.call((challenge_disposition(true), Retained::as_ptr(&credential).cast_mut()));
    }
  }

  impl AuthNavigationDelegate {
    #[unsafe(method(respondsToSelector:))]
    fn responds_to_selector(&self, selector: Sel) -> bool {
      let responds: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
      responds || self.ivars().original.respondsToSelector(selector)
    }

    #[unsafe(method(forwardingTargetForSelector:))]
    fn forwarding_target_for_selector(&self, selector: Sel) -> *mut AnyObject {
      if self.ivars().original.respondsToSelector(selector) {
        Retained::as_ptr(&self.ivars().original).cast_mut().cast()
      } else {
        unsafe { msg_send![super(self), forwardingTargetForSelector: selector] }
      }
    }
  }
);

impl AuthNavigationDelegate {
  fn track_navigation(&self, navigation: Option<&WKNavigation>) {
    if let Some(navigation) = navigation {
      let key = navigation as *const WKNavigation as usize;
      self
        .ivars()
        .navigations
        .borrow_mut()
        .entry(key)
        .or_insert_with(|| (navigation.retain(), self.ivars().messenger.generation()));
    }
  }

  fn navigation_generation(&self, navigation: Option<&WKNavigation>) -> Option<u64> {
    let key = navigation? as *const WKNavigation as usize;
    self
      .ivars()
      .navigations
      .borrow()
      .get(&key)
      .map(|(_, generation)| *generation)
  }

  fn finish_navigation(&self, navigation: Option<&WKNavigation>) -> Option<u64> {
    let key = navigation? as *const WKNavigation as usize;
    self
      .ivars()
      .navigations
      .borrow_mut()
      .remove(&key)
      .map(|(_, generation)| generation)
  }

  fn report_load_failure(&self, webview: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
    let Some(generation) = self.finish_navigation(navigation) else {
      return;
    };
    if generation != self.ivars().messenger.generation() {
      return;
    }
    let is_url_error = error.domain().isEqualToString(unsafe { NSURLErrorDomain });
    if is_url_error && error.code() == NSURLErrorCancelled {
      return;
    }
    if is_url_error && error.code() == NSURLErrorUnsupportedURL {
      let callback = error
        .userInfo()
        .objectForKey(unsafe { NSURLErrorFailingURLErrorKey })
        .and_then(|value| value.downcast::<NSURL>().ok())
        .and_then(|url| url.scheme());
      if callback.is_some_and(|scheme| scheme.to_string() == "globalprotectcallback") {
        return;
      }
    }
    let messenger = &self.ivars().messenger;
    if let Some(failure) = self.ivars().tls.borrow_mut().record_failure(webview, error, generation) {
      messenger.send_auth_event(AuthEvent::Error(generation, failure));
      return;
    }
    // Log codes rather than NSError descriptions, which may contain callback URLs.
    messenger.send_auth_event(AuthEvent::Error(
      generation,
      AuthError::LoadFailed(anyhow::anyhow!(
        "Navigation failed: {} ({})",
        error.domain(),
        error.code()
      )),
    ));
  }
}

fn navigation_delegate(webview: &WKWebView) -> Option<Retained<AuthNavigationDelegate>> {
  unsafe {
    let pointer = objc_getAssociatedObject(
      webview as *const WKWebView as *mut AnyObject,
      &AUTH_NAVIGATION_DELEGATE_KEY as *const u8 as *const c_void,
    )
    .cast::<AuthNavigationDelegate>();
    Retained::retain(pointer.cast_mut())
  }
}

fn track_requested_navigation(webview: &WKWebView, navigation: Option<&WKNavigation>) {
  let delegate = navigation_delegate(webview);
  if let Some(delegate) = delegate {
    delegate.track_navigation(navigation);
  }
}

fn challenge_disposition(is_server_trust: bool) -> NSURLSessionAuthChallengeDisposition {
  if is_server_trust {
    NSURLSessionAuthChallengeDisposition::UseCredential
  } else {
    NSURLSessionAuthChallengeDisposition::PerformDefaultHandling
  }
}

fn install_navigation_delegate(webview: &WKWebView, messenger: Arc<AuthMessenger>) -> anyhow::Result<()> {
  let original = unsafe { webview.navigationDelegate() }.context("WebView navigation delegate is unavailable")?;
  let mtm = MainThreadMarker::new().context("WebView must be configured on the main thread")?;
  let delegate = AuthNavigationDelegate::alloc(mtm).set_ivars(AuthNavigationDelegateIvars {
    original,
    messenger,
    ignore_tls_errors: Cell::new(false),
    tls: RefCell::new(tls::TlsState::default()),
    navigations: RefCell::new(HashMap::new()),
  });
  let delegate: Retained<AuthNavigationDelegate> = unsafe { msg_send![super(delegate), init] };

  unsafe {
    objc_setAssociatedObject(
      webview as *const WKWebView as *mut AnyObject,
      &AUTH_NAVIGATION_DELEGATE_KEY as *const u8 as *const c_void,
      Retained::as_ptr(&delegate).cast_mut().cast(),
      OBJC_ASSOCIATION_RETAIN_NONATOMIC,
    );
    webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use objc2_foundation::NSURLSessionAuthChallengeDisposition;

  use super::challenge_disposition;

  #[test]
  fn accepts_server_trust_challenges() {
    assert_eq!(
      challenge_disposition(true),
      NSURLSessionAuthChallengeDisposition::UseCredential
    );
  }

  #[test]
  fn preserves_default_handling_for_other_challenges() {
    assert_eq!(
      challenge_disposition(false),
      NSURLSessionAuthChallengeDisposition::PerformDefaultHandling
    );
  }
}

impl PlatformWebviewExt for PlatformWebview {
  fn install_auth_controls(&self, window: &tauri::WebviewWindow, messenger: Arc<AuthMessenger>) -> anyhow::Result<()> {
    let webview: &WKWebView = unsafe { &*self.inner().cast() };
    address_bar::install(webview)?;
    install_navigation_delegate(webview, Arc::clone(&messenger))?;
    recovery_row::install(webview, messenger)?;
    let window_handle = window.clone();
    window.on_window_event(move |event| {
      if matches!(
        event,
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
      ) {
        let _ = window_handle.with_webview(|webview| {
          let webview: &WKWebView = unsafe { &*webview.inner().cast() };
          address_bar::resize(webview);
        });
      }
    });
    Ok(())
  }

  fn set_recovery_status(&self, status: RecoveryStatus) -> anyhow::Result<()> {
    let webview: &WKWebView = unsafe { &*self.inner().cast() };
    recovery_row::update(webview, status)
  }

  fn stop_loading(&self) {
    let webview: &WKWebView = unsafe { &*self.inner().cast() };
    unsafe {
      webview.stopLoading();
    }
  }

  fn confirm_tls_exception(&self) -> anyhow::Result<()> {
    tls::confirm(unsafe { &*self.inner().cast::<WKWebView>() })
  }

  fn ignore_tls_errors(&self) -> anyhow::Result<()> {
    let webview: &WKWebView = unsafe { &*self.inner().cast() };
    let delegate = navigation_delegate(webview).context("Auth navigation delegate is unavailable")?;
    delegate.ivars().ignore_tls_errors.set(true);
    Ok(())
  }

  fn user_agent(&self) -> anyhow::Result<String> {
    unsafe {
      let wv: &NSObject = &*self.inner().cast();
      let Some(value) = wv.valueForKey(&NSString::from_str("userAgent")) else {
        bail!("Failed to get webview user agent");
      };
      let Some(user_agent) = value.downcast_ref::<NSString>() else {
        bail!("Webview user agent is not a string");
      };

      Ok(user_agent.to_string())
    }
  }

  fn set_user_agent(&self, user_agent: &str) -> anyhow::Result<()> {
    unsafe {
      let wv: &WKWebView = &*self.inner().cast();
      wv.setCustomUserAgent(Some(&NSString::from_str(user_agent)));
    }

    Ok(())
  }

  fn load_url(&self, url: &str) -> anyhow::Result<()> {
    unsafe {
      let wv: &WKWebView = &*self.inner().cast();
      let url = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| anyhow::anyhow!("Invalid URL"))?;
      let request = NSURLRequest::requestWithURL(&url);

      let navigation = wv.loadRequest(&request);
      track_requested_navigation(wv, navigation.as_deref());
    }

    Ok(())
  }

  fn load_html(&self, html: &str) -> anyhow::Result<()> {
    unsafe {
      let wv: &WKWebView = &*self.inner().cast();
      let navigation = wv.loadHTMLString_baseURL(&NSString::from_str(html), None);
      track_requested_navigation(wv, navigation.as_deref());
    }

    Ok(())
  }
}

fn navigation_address(webview: &WKWebView) -> String {
  let url = unsafe { webview.URL() }.and_then(|url| url.absoluteString());
  redact_uri(&url.map(|url| url.to_string()).unwrap_or_default())
}

fn read_navigation_html(webview: &WKWebView, messenger: Arc<AuthMessenger>, generation: u64) {
  let callback = RcBlock::new(move |body: *mut AnyObject, error: *mut NSError| {
    if let Some(error) = unsafe { error.as_ref() } {
      warn!("Failed to get html: {} ({})", error.domain(), error.code());
    } else if let Some(body) = unsafe { body.as_ref() }.and_then(|body| body.downcast_ref::<NSString>()) {
      messenger.read_from_html(&body.to_string(), generation);
    }
  });
  unsafe {
    webview.evaluateJavaScript_completionHandler(
      &NSString::from_str("document.documentElement.outerHTML"),
      Some(&callback),
    );
  }
}
