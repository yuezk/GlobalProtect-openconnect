use std::{ffi::c_void, ptr};

use anyhow::{Context, bail};
use block2::{DynBlock, RcBlock};
use objc2::{
  DefinedClass, MainThreadOnly, define_class,
  ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_setAssociatedObject},
  msg_send,
  rc::Retained,
  runtime::{AnyObject, ProtocolObject, Sel},
};
use objc2_foundation::{
  MainThreadMarker, NSError, NSObject, NSObjectNSKeyValueCoding, NSObjectProtocol, NSString, NSURL,
  NSURLAuthenticationChallenge, NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLRequest,
  NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{WKNavigationDelegate, WKWebView};
use tauri::webview::PlatformWebview;

use super::webview_auth::PlatformWebviewExt;

static TLS_NAVIGATION_DELEGATE_KEY: u8 = 0;

struct TlsNavigationDelegateIvars {
  original: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
}

define_class!(
  #[unsafe(super = NSObject)]
  #[thread_kind = MainThreadOnly]
  #[ivars = TlsNavigationDelegateIvars]
  struct TlsNavigationDelegate;

  unsafe impl NSObjectProtocol for TlsNavigationDelegate {}

  #[allow(non_snake_case)]
  unsafe impl WKNavigationDelegate for TlsNavigationDelegate {
    #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
    unsafe fn webView_didReceiveAuthenticationChallenge_completionHandler(
      &self,
      _webview: &WKWebView,
      challenge: &NSURLAuthenticationChallenge,
      completion_handler: &DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
    ) {
      let protection_space = challenge.protectionSpace();
      let is_server_trust = protection_space
        .authenticationMethod()
        .isEqualToString(unsafe { NSURLAuthenticationMethodServerTrust });

      if !is_server_trust {
        completion_handler.call((challenge_disposition(false), ptr::null_mut()));
        return;
      }

      let server_trust: *mut AnyObject = msg_send![&*protection_space, serverTrust];
      if server_trust.is_null() {
        completion_handler.call((challenge_disposition(false), ptr::null_mut()));
        return;
      }

      let credential: Retained<NSURLCredential> =
        msg_send![objc2::class!(NSURLCredential), credentialForTrust: server_trust];
      completion_handler.call((challenge_disposition(true), Retained::as_ptr(&credential).cast_mut()));
    }
  }

  impl TlsNavigationDelegate {
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

impl TlsNavigationDelegate {
  fn new(original: Retained<ProtocolObject<dyn WKNavigationDelegate>>, mtm: MainThreadMarker) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(TlsNavigationDelegateIvars { original });
    unsafe { msg_send![super(this), init] }
  }
}

fn challenge_disposition(is_server_trust: bool) -> NSURLSessionAuthChallengeDisposition {
  if is_server_trust {
    NSURLSessionAuthChallengeDisposition::UseCredential
  } else {
    NSURLSessionAuthChallengeDisposition::PerformDefaultHandling
  }
}

fn install_tls_navigation_delegate(webview: &WKWebView) -> anyhow::Result<()> {
  let original = unsafe { webview.navigationDelegate() }.context("WebView navigation delegate is unavailable")?;
  let mtm = MainThreadMarker::new().context("WebView must be configured on the main thread")?;
  let delegate = TlsNavigationDelegate::new(original, mtm);

  unsafe {
    objc_setAssociatedObject(
      webview as *const WKWebView as *mut AnyObject,
      &TLS_NAVIGATION_DELEGATE_KEY as *const u8 as *const c_void,
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
  fn ignore_tls_errors(&self) -> anyhow::Result<()> {
    let webview: &WKWebView = unsafe { &*self.inner().cast() };
    install_tls_navigation_delegate(webview)
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

      wv.loadRequest(&request);
    }

    Ok(())
  }

  fn load_html(&self, html: &str) -> anyhow::Result<()> {
    unsafe {
      let wv: &WKWebView = &*self.inner().cast();
      wv.loadHTMLString_baseURL(&NSString::from_str(html), None);
    }

    Ok(())
  }

  fn get_html(&self, callback: Box<dyn Fn(anyhow::Result<String>) + 'static>) {
    unsafe {
      let wv: &WKWebView = &*self.inner().cast();

      let js_callback = RcBlock::new(move |body: *mut AnyObject, err: *mut NSError| {
        if let Some(err) = err.as_ref() {
          let code = err.code();
          let message = err.localizedDescription();
          callback(Err(anyhow::anyhow!("Error {}: {}", code, message)));
        } else {
          let body: &NSString = &*body.cast();
          callback(Ok(body.to_string()));
        }
      });

      wv.evaluateJavaScript_completionHandler(
        &NSString::from_str("document.documentElement.outerHTML"),
        Some(&js_callback),
      );
    }
  }
}
