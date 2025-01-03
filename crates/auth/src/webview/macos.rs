use std::{borrow::Cow, sync::Arc};

use block2::RcBlock;
use objc2::{
  rc::Retained,
  runtime::{AnyObject, ProtocolObject},
};
use objc2_foundation::{NSError, NSHTTPURLResponse, NSString};
use wry::WebViewExtMacOS;

use super::{auth_messenger::AuthError, navigation_delegate::NavigationDelegate};

pub struct AuthResponse {
  response: Option<Retained<NSHTTPURLResponse>>,
  body: Option<String>,
}

impl AuthResponse {
  pub fn url(&self) -> Option<String> {
    let response = self.response.as_ref()?;
    let url = unsafe { response.URL().and_then(|url| url.absoluteString()) };

    url.map(|u| u.to_string())
  }

  pub fn get_header(&self, key: &str) -> Option<String> {
    let response = self.response.as_ref()?;
    let value = unsafe { response.valueForHTTPHeaderField(&NSString::from_str(key)) };

    value.map(|v| v.to_string())
  }

  pub fn get_body<F>(&self, cb: F)
  where
    F: FnOnce(anyhow::Result<Option<Cow<'_, str>>>) + 'static,
  {
    if let Some(body) = self.body.as_deref() {
      cb(Ok(Some(Cow::Borrowed(body))));
    } else {
      cb(Ok(None));
    }
  }
}

pub fn connect_webview_response<F>(wv: &wry::WebView, cb: F)
where
  F: Fn(anyhow::Result<AuthResponse, AuthError>) + 'static,
{
  let wv = wv.webview();
  let wv_clone = Retained::clone(&wv);

  let callback = Arc::new(cb);

  let delegate = NavigationDelegate::new(move |response| {
    let callback_clone = Arc::clone(&callback);

    if let Some(response) = response {
      callback_clone(Ok(AuthResponse {
        response: Some(response),
        body: None,
      }));
      return;
    }

    unsafe {
      let callback_clone = Arc::clone(&callback);
      let js_callback = RcBlock::new(move |body: *mut AnyObject, _err: *mut NSError| {
        let body = body as *mut NSString;
        let body = body.as_ref().unwrap();

        callback_clone(Ok(AuthResponse {
          response: None,
          body: Some(body.to_string()),
        }));
      });

      wv_clone.evaluateJavaScript_completionHandler(
        &NSString::from_str("document.documentElement.outerHTML"),
        Some(&js_callback),
      );
    }
  });

  let proto_delegate = ProtocolObject::from_ref(delegate.as_ref());
  unsafe {
    wv.setNavigationDelegate(Some(proto_delegate));
    // The UI will freeze if we don't call this method
    let _ = wv.navigationDelegate();
  };
}
