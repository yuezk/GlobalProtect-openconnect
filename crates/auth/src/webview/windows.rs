//! WebView2 authentication. All COM objects and native controls stay on the UI thread.
use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  rc::Rc,
  sync::Arc,
};

use super::{
  auth_messenger::{AuthDataLocation, AuthError, AuthEvent, AuthMessenger},
  recovery::RecoveryStatus,
  webview_auth::{GetHeader, PlatformWebviewExt},
};
use anyhow::{Context, bail};
use gpapi::auth::SamlAuthData;
use tauri::{
  AppHandle, LogicalPosition, Webview, WebviewBuilder, WebviewUrl, Window, WindowBuilder, webview::PlatformWebview,
};
use webview2_com::{
  ClearBrowsingDataCompletedHandler, CoTaskMemPWSTR, DOMContentLoadedEventHandler, ExecuteScriptCompletedHandler,
  Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler, NavigationStartingEventHandler,
  SourceChangedEventHandler, WebResourceResponseReceivedEventHandler,
};
use windows::core::{BOOL, HSTRING, Interface, PWSTR};

#[path = "windows/controls.rs"]
mod controls;
#[path = "windows/tls.rs"]
mod tls;

/// Windows hosts native chrome and a manually sized child webview in one window.
/// Host-owned child bounds keep the page surface below the native controls
/// throughout live resizing.
#[derive(Clone)]
pub(super) struct AuthWindow {
  window: Window,
  webview: Webview,
}

impl std::ops::Deref for AuthWindow {
  type Target = Window;
  fn deref(&self) -> &Window {
    &self.window
  }
}

impl AuthWindow {
  pub(super) fn with_webview(&self, callback: impl FnOnce(PlatformWebview) + Send + 'static) -> tauri::Result<()> {
    self.webview.with_webview(callback)
  }
}

impl gpapi::utils::window::WindowExt for AuthWindow {
  fn raise(&self) -> anyhow::Result<()> {
    self.window.show()?;
    self.window.set_focus()?;
    self.webview.set_focus()?;
    Ok(())
  }
}

pub(super) fn create_auth_window(
  app: &AppHandle,
  title: &str,
  theme: Option<tauri::Theme>,
) -> anyhow::Result<AuthWindow> {
  let window = WindowBuilder::new(app, "auth_window")
    .title(title)
    .theme(theme)
    .inner_size(900.0, 650.0 + super::address_bar::TOOLBAR_HEIGHT)
    .min_inner_size(400.0, 300.0)
    .focused(true)
    .visible(false)
    .center()
    .build()?;
  let webview = window.add_child(
    WebviewBuilder::new("auth_webview", WebviewUrl::default())
      .devtools(cfg!(any(debug_assertions, feature = "release-devtools"))),
    LogicalPosition::new(0.0, 0.0),
    window.inner_size()?,
  )?;
  Ok(AuthWindow { window, webview })
}

thread_local! {
  static STATES: RefCell<HashMap<usize, Rc<State>>> = RefCell::default();
}

struct State {
  core: ICoreWebView2,
  controller: ICoreWebView2Controller,
  messenger: Arc<AuthMessenger>,
  controls: controls::Controls,
  pending: Cell<Option<u64>>,
  navigations: RefCell<HashMap<u64, u64>>,
  document: RefCell<(String, u64)>,
  tls: RefCell<tls::Policy>,
}

fn text(get: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> windows::core::Result<String> {
  let mut value = PWSTR::null();
  let result = get(&mut value);
  let value = CoTaskMemPWSTR::from(value);
  result?;
  Ok(value.to_string())
}

fn state(webview: &PlatformWebview) -> anyhow::Result<Rc<State>> {
  let core = unsafe { webview.controller().CoreWebView2()? };
  STATES
    .with_borrow(|states| states.get(&(core.as_raw() as usize)).cloned())
    .context("Authentication controls are unavailable")
}

impl State {
  fn read_html(&self, generation: u64) -> windows::core::Result<()> {
    if generation != self.messenger.generation() {
      return Ok(());
    }
    let messenger = Arc::clone(&self.messenger);
    unsafe {
      self.core.ExecuteScript(
        &HSTRING::from("document.documentElement.outerHTML"),
        &ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
          result?;
          if let Ok(html) = serde_json::from_str::<String>(&json) {
            messenger.read_from_html(&html, generation);
          }
          Ok(())
        })),
      )
    }
  }

  fn listen(self: &Rc<Self>) -> windows::core::Result<()> {
    let mut token = 0;
    let weak = Rc::downgrade(self);
    let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
      let (Some(state), Some(args)) = (weak.upgrade(), args) else {
        return Ok(());
      };
      unsafe {
        let mut id = 0;
        args.NavigationId(&mut id)?;
        let generation = *state
          .navigations
          .borrow_mut()
          .entry(id)
          .or_insert_with(|| state.pending.take().unwrap_or_else(|| state.messenger.generation()));
        let uri = text(|p| args.Uri(p))?;
        if uri.starts_with("globalprotectcallback:") {
          args.SetCancel(true)?;
          match SamlAuthData::from_gpcallback(&uri) {
            Ok(data) => state
              .messenger
              .send_auth_event(AuthEvent::Data(generation, data, AuthDataLocation::Body)),
            Err(error) => state
              .messenger
              .send_auth_event(AuthEvent::Error(generation, AuthError::invalid_from_body(error.into()))),
          }
        } else if generation == state.messenger.generation() {
          *state.document.borrow_mut() = (uri, generation);
          state.messenger.cancel_raise_window();
        }
      }
      Ok(())
    }));
    unsafe {
      self.core.add_NavigationStarting(&starting, &mut token)?;
    }

    let weak = Rc::downgrade(self);
    let loaded = DOMContentLoadedEventHandler::create(Box::new(move |_, args| {
      let (Some(state), Some(args)) = (weak.upgrade(), args) else {
        return Ok(());
      };
      let mut id = 0;
      unsafe {
        args.NavigationId(&mut id)?;
      }
      let generation = state.navigations.borrow().get(&id).copied();
      if let Some(generation) = generation {
        state.read_html(generation)?;
      }
      Ok(())
    }));
    unsafe {
      self
        .core
        .cast::<ICoreWebView2_2>()?
        .add_DOMContentLoaded(&loaded, &mut token)?;
    }

    let weak = Rc::downgrade(self);
    let completed = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
      let (Some(state), Some(args)) = (weak.upgrade(), args) else {
        return Ok(());
      };
      unsafe {
        let mut id = 0;
        args.NavigationId(&mut id)?;
        let Some(generation) = state.navigations.borrow_mut().remove(&id) else {
          return Ok(());
        };
        let mut success = BOOL(0);
        args.IsSuccess(&mut success)?;
        if success.as_bool() {
          state.read_html(generation)?;
          state.messenger.send_auth_event(AuthEvent::PageLoaded(generation));
        } else {
          let mut error = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
          args.WebErrorStatus(&mut error)?;
          if error != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
            state.messenger.send_auth_event(AuthEvent::Error(
              generation,
              AuthError::LoadFailed(anyhow::anyhow!("WebView2 navigation failed ({})", error.0)),
            ));
          }
        }
      }
      Ok(())
    }));
    unsafe {
      self.core.add_NavigationCompleted(&completed, &mut token)?;
    }

    let weak = Rc::downgrade(self);
    let source = SourceChangedEventHandler::create(Box::new(move |_, _| {
      if let Some(state) = weak.upgrade() {
        let uri = unsafe { text(|p| state.core.Source(p))? };
        state.controls.set_address(&uri)?;
      }
      Ok(())
    }));
    unsafe {
      self.core.add_SourceChanged(&source, &mut token)?;
    }

    let weak = Rc::downgrade(self);
    let response = WebResourceResponseReceivedEventHandler::create(Box::new(move |_, args| {
      let (Some(state), Some(args)) = (weak.upgrade(), args) else {
        return Ok(());
      };
      unsafe {
        let uri = text(|p| args.Request()?.Uri(p))?;
        let (document, generation) = state.document.borrow().clone();
        // Subresources must not supply authentication credentials for the page.
        if uri == document && generation == state.messenger.generation() {
          state
            .messenger
            .read_from_response(&Headers(args.Response()?.Headers()?), generation);
        }
      }
      Ok(())
    }));
    unsafe {
      self
        .core
        .cast::<ICoreWebView2_2>()?
        .add_WebResourceResponseReceived(&response, &mut token)?;
    }
    tls::install(self)?;
    Ok(())
  }
}

struct Headers(ICoreWebView2HttpResponseHeaders);
impl GetHeader for Headers {
  fn get_header(&self, key: &str) -> Option<String> {
    unsafe { text(|p| self.0.GetHeader(&HSTRING::from(key), p)).ok() }
  }
}

impl PlatformWebviewExt for PlatformWebview {
  fn install_auth_controls(&self, window: &AuthWindow, messenger: Arc<AuthMessenger>) -> anyhow::Result<()> {
    let controller = self.controller();
    let core = unsafe { controller.CoreWebView2()? };
    let key = core.as_raw() as usize;
    let controls = controls::Controls::new(window, &controller, key)?;
    let state = Rc::new(State {
      core,
      controller,
      messenger,
      controls,
      pending: Cell::new(None),
      navigations: RefCell::default(),
      document: RefCell::new((String::new(), 0)),
      tls: RefCell::default(),
    });
    STATES.with_borrow_mut(|states| states.insert(key, Rc::clone(&state)));
    state.listen()?;
    state.controls.layout(&state)?;
    Ok(())
  }

  fn set_recovery_status(&self, status: RecoveryStatus) -> anyhow::Result<()> {
    let state = state(self)?;
    state.controls.update(&state, status)?;
    Ok(())
  }
  fn stop_loading(&self) {
    let _ = unsafe { self.controller().CoreWebView2().and_then(|wv| wv.Stop()) };
  }
  fn confirm_tls_exception(&self) -> anyhow::Result<()> {
    tls::confirm(state(self)?.as_ref())
  }
  fn ignore_tls_errors(&self) -> anyhow::Result<()> {
    state(self)?.tls.borrow_mut().ignore_errors = true;
    Ok(())
  }
  fn user_agent(&self) -> anyhow::Result<String> {
    unsafe {
      Ok(text(|p| {
        self
          .controller()
          .CoreWebView2()?
          .Settings()?
          .cast::<ICoreWebView2Settings2>()?
          .UserAgent(p)
      })?)
    }
  }
  fn set_user_agent(&self, user_agent: &str) -> anyhow::Result<()> {
    unsafe {
      self
        .controller()
        .CoreWebView2()?
        .Settings()?
        .cast::<ICoreWebView2Settings2>()?
        .SetUserAgent(&HSTRING::from(user_agent))?;
    }
    Ok(())
  }
  fn load_url(&self, url: &str) -> anyhow::Result<()> {
    let state = state(self)?;
    state.pending.set(Some(state.messenger.generation()));
    unsafe {
      state.core.Navigate(&HSTRING::from(url))?;
    }
    Ok(())
  }
  fn load_html(&self, html: &str) -> anyhow::Result<()> {
    let state = state(self)?;
    state.pending.set(Some(state.messenger.generation()));
    unsafe {
      state.core.NavigateToString(&HSTRING::from(html))?;
    }
    Ok(())
  }
}

/// WebView2 clears profiles asynchronously; do not navigate until cookies are gone.
pub(super) async fn clear_browsing_data(window: &AuthWindow) -> anyhow::Result<()> {
  let (tx, rx) = tokio::sync::oneshot::channel();
  window.with_webview(move |wv| {
    let tx = Rc::new(RefCell::new(Some(tx)));
    let complete = Rc::clone(&tx);
    let callback = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
      if let Some(tx) = complete.borrow_mut().take() {
        let _ = tx.send(result);
      }
      Ok(())
    }));
    let result = (|| unsafe {
      wv.controller()
        .CoreWebView2()?
        .cast::<ICoreWebView2_13>()?
        .Profile()?
        .cast::<ICoreWebView2Profile2>()?
        .ClearBrowsingDataAll(&callback)
    })();
    if let Err(error) = result {
      if let Some(tx) = tx.borrow_mut().take() {
        let _ = tx.send(Err(error));
      }
    }
  })?;
  rx.await.context("Could not clear sign-in data")??;
  Ok(())
}
