use std::ffi::c_void;

use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{NSHTTPURLResponse, NSString, NSURLRequest, NSURL};
use wry::WryWebView;

use crate::webview_auth::auth_response::read_auth_data;

use super::{
  auth_response::AuthResponse,
  auth_settings::{AuthRequest, AuthSettings},
  navigation_delegate::NavigationDelegate,
};

fn cast_webview(wv: &*mut c_void) -> Retained<WryWebView> {
  let wry_webview = unsafe { Retained::from_raw(*wv as *mut WryWebView) };
  wry_webview.expect("Failed to get webview")
}

impl AuthResponse for Retained<NSHTTPURLResponse> {
  fn url(&self) -> Option<String> {
    let url = unsafe { self.URL().and_then(|url| url.absoluteString()) };
    url.map(|u| u.to_string())
  }

  fn get_header(&self, key: &str) -> Option<String> {
    let value = unsafe { self.valueForHTTPHeaderField(&NSString::from_str(key)) };
    value.map(|v| v.to_string())
  }

  fn get_body<F>(&self, cb: F)
  where
    F: FnOnce(anyhow::Result<Vec<u8>>) + 'static,
  {
  }
}

pub(crate) fn setup_webview(wv: &*mut c_void, auth_settings: AuthSettings<'_>) -> anyhow::Result<()> {
  let wry_webview = cast_webview(wv);
  let AuthSettings {
    auth_request,
    auth_messenger,
    ..
  } = auth_settings;

  let auth_messenger_clone = auth_messenger.clone();
  let delegate = NavigationDelegate::new(move |response| {
    read_auth_data(&response, &auth_messenger_clone);
  });

  let proto_delegate = ProtocolObject::from_ref(delegate.as_ref());

  unsafe { wry_webview.setNavigationDelegate(Some(proto_delegate)) };

  load_auth_request(wv, &auth_request);
  Ok(())
}

pub(crate) fn load_auth_request(wv: &*mut c_void, auth_request: &AuthRequest<'_>) {
  let wry_webview = cast_webview(wv);

  println!("Loading auth request: {:?}", auth_request);

  if auth_request.is_url() {
    unsafe {
      let url = NSURL::URLWithString(&NSString::from_str(auth_request.as_str())).unwrap();
      println!("Loading URL: {:?}", url);
      let request = NSURLRequest::requestWithURL(&url);
      wry_webview.loadRequest(&request);
    }
  } else {
    unsafe {
      wry_webview.loadHTMLString_baseURL(&NSString::from_str(auth_request.as_str()), None);
    }
  }
}

pub(crate) fn clear_data<F>(wv: &*mut c_void, cb: F)
where
  F: FnOnce(anyhow::Result<()>) + Send + 'static,
{
  todo!()
}
