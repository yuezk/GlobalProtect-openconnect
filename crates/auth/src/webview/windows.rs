use log::warn;
use tauri::webview::PlatformWebview;
use webview2_com::{
  pwstr_from_str, take_pwstr, ExecuteScriptCompletedHandler,
  Microsoft::Web::WebView2::Win32::{
    ICoreWebView2WebResourceResponseView, ICoreWebView2_14, ICoreWebView2_2,
    COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW,
  },
  ServerCertificateErrorDetectedEventHandler, WebResourceResponseReceivedEventHandler,
};
use windows_core::{Interface, PWSTR};

use super::{
  auth_messenger::AuthError,
  webview_auth::{GetHeader, PlatformWebviewExt},
};

impl PlatformWebviewExt for PlatformWebview {
  fn ignore_tls_errors(&self) -> anyhow::Result<()> {
    unsafe {
      let wv = self.controller().CoreWebView2()?.cast::<ICoreWebView2_14>()?;
      let handler = ServerCertificateErrorDetectedEventHandler::create(Box::new(|_, e| {
        if let Some(e) = e {
          let _ = e.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW);
        }
        Ok(())
      }));

      wv.add_ServerCertificateErrorDetected(&handler, &mut Default::default())?;
    }

    Ok(())
  }

  fn load_url(&self, url: &str) -> anyhow::Result<()> {
    let url = pwstr_from_str(url);

    unsafe { self.controller().CoreWebView2()?.Navigate(url)? }

    Ok(())
  }

  fn load_html(&self, html: &str) -> anyhow::Result<()> {
    let html = pwstr_from_str(html);

    unsafe { self.controller().CoreWebView2()?.NavigateToString(html)? }

    Ok(())
  }

  fn get_html(&self, callback: Box<dyn Fn(anyhow::Result<String>) + 'static>) {
    unsafe {
      match self.controller().CoreWebView2() {
        Ok(wv) => {
          let js = "document.documentElement.outerHTML";
          let js = pwstr_from_str(js);

          let handler = ExecuteScriptCompletedHandler::create(Box::new(move |err, html| {
            if let Err(err) = err {
              callback(Err(anyhow::anyhow!(err)));
              return Ok(());
            }

            // The returned HTML is JSON.stringify'd string, so we need to parse it
            let res = match serde_json::from_str(&html) {
              Ok(Some(html)) => Ok(html),
              Ok(None) => Err(anyhow::anyhow!("No HTML returned")),
              Err(err) => Err(anyhow::anyhow!(err)),
            };
            callback(res);

            Ok(())
          }));

          if let Err(err) = wv.ExecuteScript(js, &handler) {
            warn!("Failed to execute script: {}", err);
          }
        }
        Err(err) => callback(Err(anyhow::anyhow!(err))),
      }
    }
  }
}

impl GetHeader for ICoreWebView2WebResourceResponseView {
  fn get_header(&self, key: &str) -> Option<String> {
    unsafe {
      let headers = self.Headers().ok()?;
      let key = pwstr_from_str(key);

      let mut contains = Default::default();
      headers.Contains(key, &mut contains).ok()?;

      if contains.as_bool() {
        let mut value = PWSTR::null();
        headers.GetHeader(key, &mut value).ok()?;
        let value = take_pwstr(value);

        Some(value)
      } else {
        None
      }
    }
  }
}

pub trait PlatformWebviewOnResponse {
  fn on_response(
    &self,
    callback: Box<dyn Fn(anyhow::Result<ICoreWebView2WebResourceResponseView, AuthError>) + 'static>,
  );
}

impl PlatformWebviewOnResponse for PlatformWebview {
  fn on_response(
    &self,
    callback: Box<dyn Fn(anyhow::Result<ICoreWebView2WebResourceResponseView, AuthError>) + 'static>,
  ) {
    unsafe {
      let _ = self
        .controller()
        .CoreWebView2()
        .and_then(|wv| wv.cast::<ICoreWebView2_2>())
        .map(|wv| {
          let handler = WebResourceResponseReceivedEventHandler::create(Box::new(move |_, e| {
            let Some(e) = e else {
              return Ok(());
            };

            match e.Response() {
              Ok(res) => callback(Ok(res)),
              Err(err) => warn!("Failed to get response: {}", err),
            }

            Ok(())
          }));

          let _ = wv.add_WebResourceResponseReceived(&handler, &mut Default::default());
        });
    }
  }
}
