use std::borrow::Cow;

use gpapi::{auth::SamlAuthData, gp_params::GpParams};
use tao::{
  event_loop::EventLoop,
  window::{Window, WindowBuilder},
};
use wry::WebViewBuilder;

use crate::{auth_prelogin, webview::auth_response::read_auth_data};

use super::platform_impl::connect_webview_response;

pub struct WebviewAuthenticator<'a> {
  server: &'a str,
  gp_params: &'a GpParams,
  auth_request: Option<&'a str>,
  clean: bool,

  window: Window,
  webview: wry::WebView,
  // response_reader: AuthResponseReader,
  // pub(crate) window: Option<tao::window::Window>,
  // #[cfg(feature = "webview-auth")]
  // pub(crate) response_reader: Option<Box<dyn ResponseReader>>,

  // pub(crate) is_retrying: tokio::sync::RwLock<bool>,
}

impl<'a> WebviewAuthenticator<'a> {
  pub fn builder(server: &'a str, gp_params: &'a GpParams) -> WebviewAuthenticatorBuilder<'a> {
    WebviewAuthenticatorBuilder {
      server,
      gp_params,
      auth_request: None,
      clean: false,
    }
  }

  pub async fn authenticate(&self) -> anyhow::Result<()> {
    let auth_request = match self.auth_request {
      Some(auth_request) => Cow::Borrowed(auth_request),
      None => Cow::Owned(auth_prelogin(&self.server, &self.gp_params).await?),
    };

    if auth_request.starts_with("http") {
      self.webview.load_url(&auth_request)?;
    } else {
      self.webview.load_html(&auth_request)?;
    }

    Ok(())
  }
}

pub struct WebviewAuthenticatorBuilder<'a> {
  server: &'a str,
  gp_params: &'a GpParams,
  auth_request: Option<&'a str>,
  clean: bool,
}

impl<'a> WebviewAuthenticatorBuilder<'a> {
  pub fn auth_request(mut self, auth_request: &'a str) -> Self {
    self.auth_request = Some(auth_request);
    self
  }

  pub fn clean(mut self, clean: bool) -> Self {
    self.clean = clean;
    self
  }

  pub fn build(
    self,
    event_loop: &'a EventLoop<anyhow::Result<SamlAuthData>>,
  ) -> anyhow::Result<WebviewAuthenticator<'a>> {
    let window = WindowBuilder::new()
      .with_title("GlobalProtect Authentication")
      .with_focused(true)
      .build(event_loop)?;

    let builder = WebViewBuilder::new();

    #[cfg(not(target_os = "macos"))]
    let webview = {
      use tao::platform::unix::WindowExtUnix;
      use wry::WebViewBuilderExtUnix;
      let vbox = window
        .default_vbox()
        .ok_or_else(|| anyhow::anyhow!("Failed to get default vbox"))?;
      builder.build_gtk(vbox)?
    };

    connect_webview_response(&webview, |response| {
      // println!("Received response: {:?}", response.unwrap().url());
      match response {
        Ok(response) => read_auth_data(response, |auth_result| {
          println!("Auth result: {:?}", auth_result);
        }),
        Err(err) => todo!(),
      }
    });

    // let event_proxy = event_loop.create_proxy();

    // let response_reader = AuthResponseReader::new(&webview).on_response(move |response| {
    //   // println!("Received response: {:?}", response.unwrap().url());
    //   match response {
    //     Ok(response) => read_auth_data(response, |auth_result| {
    //       // println!("Auth result: {:?}", auth_result);
    //     }),
    //     Err(err) => todo!(),
    //   }
    // });

    Ok(WebviewAuthenticator {
      server: self.server,
      gp_params: self.gp_params,
      auth_request: None,
      clean: false,
      window,
      webview,
      // response_reader,
    })
  }
}
