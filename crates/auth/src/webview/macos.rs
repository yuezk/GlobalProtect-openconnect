use block2::RcBlock;
use log::warn;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSError, NSString, NSURLRequest, NSURL};
use objc2_web_kit::WKWebView;
use tauri::webview::PlatformWebview;

use super::webview_auth::PlatformWebviewExt;

impl PlatformWebviewExt for PlatformWebview {
  fn ignore_tls_errors(&self) -> anyhow::Result<()> {
    warn!("Ignoring TLS errors is not supported on macOS");
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
