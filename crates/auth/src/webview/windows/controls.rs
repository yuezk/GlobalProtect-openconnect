//! Native controls remain outside the identity provider's document.
use super::*;
use windows::{
  Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    UI::{
      Controls::EM_SETRECT,
      HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi},
      Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
      WindowsAndMessaging::*,
    },
  },
  core::{PCWSTR, w},
};

const SUBCLASS: usize = 0x47504155;
const ADDRESS_SUBCLASS: usize = 0x47504156;
const ADDRESS_HEIGHT: i32 = 24;
const TOOLBAR_HEIGHT: i32 = 34;
const RETRY: usize = 101;
const PROCEED: usize = 102;
const CANCEL: usize = 103;

pub(super) struct Controls {
  pub(super) parent: HWND,
  webview_host: HWND,
  address: HWND,
  message: HWND,
  retry: HWND,
  proceed: HWND,
  cancel: HWND,
  status: Cell<RecoveryStatus>,
  font: Cell<Option<(u32, HFONT)>>,
}

impl Controls {
  pub(super) fn new(window: &AuthWindow, controller: &ICoreWebView2Controller, key: usize) -> anyhow::Result<Self> {
    // The host owns both the toolbar and the child webview's remaining area.
    let parent = HWND(window.hwnd()?.0);
    let mut webview_host = HWND::default();
    unsafe {
      controller.ParentWindow(&mut webview_host)?;
    }
    let child = |class: PCWSTR, title: &str, style: WINDOW_STYLE, id: usize| -> windows::core::Result<HWND> {
      unsafe {
        let hwnd = CreateWindowExW(
          WINDOW_EX_STYLE::default(),
          class,
          &HSTRING::from(title),
          WS_CHILD | style,
          0,
          0,
          0,
          0,
          Some(parent),
          Some(HMENU(id as *mut _)),
          None,
          None,
        )?;
        Ok(hwnd)
      }
    };
    let controls = Self {
      parent,
      webview_host,
      address: child(
        w!("EDIT"),
        super::super::address_bar::LOADING_ADDRESS,
        WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_READONLY as u32 | ES_MULTILINE as u32 | ES_AUTOHSCROLL as u32),
        0,
      )?,
      message: child(w!("STATIC"), "", WINDOW_STYLE::default(), 0)?,
      retry: child(w!("BUTTON"), "Try Again", WS_TABSTOP, RETRY)?,
      proceed: child(w!("BUTTON"), "Continue Anyway…", WS_TABSTOP, PROCEED)?,
      cancel: child(w!("BUTTON"), "Cancel", WS_TABSTOP, CANCEL)?,
      status: Cell::new(RecoveryStatus::Hidden),
      font: Cell::new(None),
    };
    unsafe {
      anyhow::ensure!(
        SetWindowSubclass(controls.address, Some(address_proc), ADDRESS_SUBCLASS, 0).as_bool(),
        "Could not style the sign-in address"
      );
      anyhow::ensure!(
        SetWindowSubclass(parent, Some(window_proc), SUBCLASS, key).as_bool(),
        "Could not install sign-in controls"
      );
    }
    Ok(controls)
  }

  pub(super) fn set_address(&self, uri: &str) -> windows::core::Result<()> {
    unsafe {
      SetWindowTextW(
        self.address,
        &HSTRING::from(super::super::address_bar::display_address(Some(uri))),
      )
    }
  }

  pub(super) fn update(&self, state: &State, status: RecoveryStatus) -> windows::core::Result<()> {
    self.status.set(status);
    let message = if status.detail().is_empty() {
      status.message().to_owned()
    } else {
      format!("{}\r\n{}", status.message(), status.detail())
    };
    unsafe {
      SetWindowTextW(self.message, &HSTRING::from(message))?;
      for (hwnd, visible) in [
        (self.message, status != RecoveryStatus::Hidden),
        (self.retry, status.can_retry()),
        (self.proceed, status.can_continue()),
        (self.cancel, status.blocks_page()),
      ] {
        let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
      }
      state.controller.SetIsVisible(!status.blocks_page())?;
    }
    self.layout(state)
  }

  pub(super) fn layout(&self, state: &State) -> windows::core::Result<()> {
    unsafe {
      let dpi = GetDpiForWindow(self.parent);
      let scale = dpi as f64 / 96.0;
      let px = |value: i32| (value as f64 * scale).round() as i32;
      let mut bounds = RECT::default();
      GetClientRect(self.parent, &mut bounds)?;
      let width = bounds.right;
      if self.font.get().is_none_or(|(current_dpi, _)| current_dpi != dpi) {
        let mut metrics = NONCLIENTMETRICSW {
          cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
          ..Default::default()
        };
        SystemParametersInfoForDpi(
          SPI_GETNONCLIENTMETRICS.0,
          metrics.cbSize,
          Some(&mut metrics as *mut _ as *mut _),
          0,
          dpi,
        )?;
        metrics.lfMessageFont.lfHeight = -px(13);
        let font = CreateFontIndirectW(&metrics.lfMessageFont);
        if font.is_invalid() {
          return Err(windows::core::Error::from_thread());
        }
        for control in [self.address, self.message, self.retry, self.proceed, self.cancel] {
          SendMessageW(control, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        }
        if let Some((_, old)) = self.font.replace(Some((dpi, font))) {
          let _ = DeleteObject(old.into());
        }
      }
      MoveWindow(
        self.address,
        px(8),
        px(5),
        (width - px(16)).max(0),
        px(ADDRESS_HEIGHT),
        true,
      )?;
      // A one-line formatting rectangle centers the native read-only edit text.
      // ES_AUTOHSCROLL keeps long URLs on one line and lets users select/copy them.
      let mut client = RECT::default();
      GetClientRect(self.address, &mut client)?;
      let dc = GetDC(Some(self.address));
      let previous = SelectObject(dc, self.font.get().expect("font initialized").1.into());
      let mut text_metrics = TEXTMETRICW::default();
      let measured = GetTextMetricsW(dc, &mut text_metrics);
      SelectObject(dc, previous);
      ReleaseDC(Some(self.address), dc);
      measured.ok()?;
      let top = ((client.bottom - text_metrics.tmHeight) / 2).max(0);
      let text = RECT {
        left: px(6),
        top,
        right: client.right - px(6),
        bottom: top + text_metrics.tmHeight,
      };
      SendMessageW(
        self.address,
        EM_SETRECT,
        Some(WPARAM(0)),
        Some(LPARAM(&text as *const _ as isize)),
      );
      let status = self.status.get();
      let header = if status == RecoveryStatus::Hidden {
        TOOLBAR_HEIGHT
      } else {
        TOOLBAR_HEIGHT + 68
      };
      MoveWindow(
        self.message,
        px(8),
        px(TOOLBAR_HEIGHT + 2),
        (width - px(16)).max(0),
        px(38),
        true,
      )?;
      let mut right = width - px(8);
      for (hwnd, visible, size) in [
        (self.cancel, status.blocks_page(), 70),
        (self.proceed, status.can_continue(), 136),
        (self.retry, status.can_retry(), 84),
      ] {
        if visible {
          right -= px(size);
          MoveWindow(hwnd, right, px(TOOLBAR_HEIGHT + 38), px(size), px(28), true)?;
          right -= px(6);
        }
      }
      let content_height = (bounds.bottom - px(header)).max(0);
      MoveWindow(self.webview_host, 0, px(header), width, content_height, true)?;
      state.controller.SetBounds(RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: content_height,
      })?;
      let _ = InvalidateRect(Some(self.parent), None, true);
    }
    Ok(())
  }
}

impl Drop for Controls {
  fn drop(&mut self) {
    if let Some((_, font)) = self.font.get() {
      unsafe {
        let _ = DeleteObject(font.into());
      }
    }
  }
}

/// Keep the native edit's selection and accessibility, with a quiet, flat frame.
unsafe extern "system" fn address_proc(
  hwnd: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
  id: usize,
  _: usize,
) -> LRESULT {
  if msg == WM_NCDESTROY {
    unsafe {
      let _ = RemoveWindowSubclass(hwnd, Some(address_proc), id);
    }
  }
  let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
  // EDIT also repaints its frame during WM_PAINT, so style it after native drawing.
  if matches!(msg, WM_NCPAINT | WM_PAINT) {
    unsafe {
      let dc = GetWindowDC(Some(hwnd));
      let mut bounds = RECT::default();
      if !dc.is_invalid() && GetWindowRect(hwnd, &mut bounds).is_ok() {
        let background = GetSysColor(COLOR_3DFACE);
        let foreground = GetSysColor(COLOR_WINDOWTEXT);
        let mut border = 0;
        for shift in [0, 8, 16] {
          let channel = (((background >> shift) & 255) * 82 + ((foreground >> shift) & 255) * 18) / 100;
          border |= channel << shift;
        }
        let brush = CreateSolidBrush(COLORREF(border));
        FrameRect(
          dc,
          &RECT {
            left: 0,
            top: 0,
            right: bounds.right - bounds.left,
            bottom: bounds.bottom - bounds.top,
          },
          brush,
        );
        let _ = DeleteObject(brush.into());
      }
      ReleaseDC(Some(hwnd), dc);
    }
  }
  result
}

unsafe extern "system" fn window_proc(
  hwnd: HWND,
  msg: u32,
  wparam: WPARAM,
  lparam: LPARAM,
  id: usize,
  key: usize,
) -> LRESULT {
  let state = STATES.with_borrow(|states| states.get(&key).cloned());
  // Paint the native toolbar and recovery surface outside the child webview.
  if msg == WM_ERASEBKGND {
    unsafe {
      let mut bounds = RECT::default();
      if GetClientRect(hwnd, &mut bounds).is_ok() {
        FillRect(HDC(wparam.0 as *mut _), &bounds, GetSysColorBrush(COLOR_WINDOW));
      }
    }
    return LRESULT(1);
  }
  if let Some(state) = &state {
    if msg == WM_COMMAND && (wparam.0 >> 16) == BN_CLICKED as usize {
      let event = match wparam.0 & 0xffff {
        RETRY => Some(AuthEvent::Retry),
        PROCEED => Some(AuthEvent::RequestTlsException),
        CANCEL => Some(AuthEvent::Close),
        _ => None,
      };
      if let Some(event) = event {
        state.messenger.send_auth_event(event);
        return LRESULT(0);
      }
    }
  }
  let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
  if msg == WM_SIZE || msg == WM_DPICHANGED {
    if let Some(state) = &state {
      let _ = state.controls.layout(state);
    }
  }
  if msg == WM_NCDESTROY {
    unsafe {
      let _ = RemoveWindowSubclass(hwnd, Some(window_proc), id);
    }
    STATES.with_borrow_mut(|states| states.remove(&key));
    if let Some(state) = state {
      super::tls::clear_exceptions(&state.core);
    }
  }
  result
}
