use log::{debug, info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tauri::EventHandler;
use tauri::{AppHandle, Manager, Window, WindowBuilder, WindowEvent::CloseRequested, WindowUrl};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use webkit2gtk::gio::Cancellable;
use webkit2gtk::glib::GString;
use webkit2gtk::traits::{URIResponseExt, WebViewExt};
use webkit2gtk::{CookieManagerExt, LoadEvent, WebContextExt, WebResource, WebResourceExt};

const AUTH_WINDOW_LABEL: &str = "auth_window";
const AUTH_ERROR_EVENT: &str = "auth-error";
const AUTH_REQUEST_EVENT: &str = "auth-request";
// Timeout to show the window if the token is not found in the response
// It will be cancelled if the token is found in the response
const SHOW_WINDOW_TIMEOUT: u64 = 3;
// A fallback timeout to show the window in case the authentication process takes longer than expected
const FALLBACK_SHOW_WINDOW_TIMEOUT: u64 = 15;

#[derive(Debug, Clone, Deserialize)]
pub(crate) enum SamlBinding {
    #[serde(rename = "REDIRECT")]
    Redirect,
    #[serde(rename = "POST")]
    Post,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AuthRequest {
    #[serde(alias = "samlBinding")]
    saml_binding: SamlBinding,
    #[serde(alias = "samlRequest")]
    saml_request: String,
}

impl AuthRequest {
    pub fn new(saml_binding: SamlBinding, saml_request: String) -> Self {
        Self {
            saml_binding,
            saml_request,
        }
    }
}

impl TryFrom<Option<&str>> for AuthRequest {
    type Error = serde_json::Error;

    fn try_from(value: Option<&str>) -> Result<Self, Self::Error> {
        serde_json::from_str(value.unwrap_or("{}"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AuthData {
    username: Option<String>,
    prelogin_cookie: Option<String>,
    portal_userauthcookie: Option<String>,
}

impl AuthData {
    fn new(
        username: Option<String>,
        prelogin_cookie: Option<String>,
        portal_userauthcookie: Option<String>,
    ) -> Self {
        Self {
            username,
            prelogin_cookie,
            portal_userauthcookie,
        }
    }

    fn check(&self) -> bool {
        self.username.is_some()
            && (self.prelogin_cookie.is_some() || self.portal_userauthcookie.is_some())
    }
}

#[derive(Debug)]
enum AuthError {
    TokenNotFound,
    TokenInvalid,
}

#[derive(Debug)]
enum AuthEvent {
    Request(AuthRequest),
    Success(AuthData),
    Error(AuthError),
    Cancel,
}

pub(crate) async fn saml_login(
    auth_request: AuthRequest,
    ua: &str,
    clear_cookies: bool,
    app_handle: &AppHandle,
) -> tauri::Result<Option<AuthData>> {
    info!("Starting SAML login");

    let (event_tx, event_rx) = mpsc::channel::<AuthEvent>(8);
    let window = build_window(app_handle, ua)?;
    setup_webview(&window, clear_cookies, event_tx.clone())?;
    let handler = setup_window(&window, event_tx);

    let result = process(&window, auth_request, event_rx).await;
    window.unlisten(handler);
    result
}

fn build_window(app_handle: &AppHandle, ua: &str) -> tauri::Result<Window> {
    let url = WindowUrl::App("auth.html".into());
    WindowBuilder::new(app_handle, AUTH_WINDOW_LABEL, url)
        .visible(false)
        .title("GlobalProtect Login")
        .user_agent(ua)
        .always_on_top(true)
        .focused(true)
        .center()
        .build()
}

// Setup webview events
fn setup_webview(
    window: &Window,
    clear_cookies: bool,
    event_tx: mpsc::Sender<AuthEvent>,
) -> tauri::Result<()> {
    window.with_webview(move |wv| {
        let wv = wv.inner();
        let event_tx = event_tx.clone();

        if clear_cookies {
            clear_webview_cookies(&wv);
        }

        wv.connect_load_changed(move |wv, event| {
            if LoadEvent::Finished != event {
                return;
            }

            let uri = wv.uri().unwrap_or("".into());
            // Empty URI indicates that an error occurred
            if uri.is_empty() {
                warn!("Empty URI loaded");
                if let Err(err) = event_tx.blocking_send(AuthEvent::Error(AuthError::TokenInvalid))
                {
                    warn!("Error sending event: {}", err);
                }
                return;
            }

            // TODO, redact URI
            debug!("Loaded URI: {}", uri);

            if let Some(main_res) = wv.main_resource() {
                parse_auth_data(&main_res, event_tx.clone());
            } else {
                warn!("No main_resource");
            }
        });

        wv.connect_load_failed(|_wv, event, err_msg, err| {
            warn!("Load failed: {:?}, {}, {:?}", event, err_msg, err);
            false
        });
    })
}

fn setup_window(window: &Window, event_tx: mpsc::Sender<AuthEvent>) -> EventHandler {
    let event_tx_clone = event_tx.clone();
    window.on_window_event(move |event| {
        if let CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Err(err) = event_tx_clone.blocking_send(AuthEvent::Cancel) {
                warn!("Error sending event: {}", err)
            }
        }
    });

    window.listen_global(AUTH_REQUEST_EVENT, move |event| {
        if let Ok(payload) = TryInto::<AuthRequest>::try_into(event.payload()) {
            let event_tx = event_tx.clone();
            let _ = tokio::spawn(async move {
                if let Err(err) = event_tx.send(AuthEvent::Request(payload)).await {
                    warn!("Error sending event: {}", err);
                }
            });
        }
    })
}

async fn process(
    window: &Window,
    auth_request: AuthRequest,
    event_rx: mpsc::Receiver<AuthEvent>,
) -> tauri::Result<Option<AuthData>> {
    info!("Processing auth request: {:?}", auth_request);

    process_request(window, auth_request)?;

    let handle = tokio::spawn(show_window_after_timeout(window.clone()));
    let auth_data = process_auth_event(&window, event_rx).await;
    handle.abort();
    Ok(auth_data)
}

fn process_request(window: &Window, auth_request: AuthRequest) -> tauri::Result<()> {
    let saml_request = auth_request.saml_request;
    let is_post = matches!(auth_request.saml_binding, SamlBinding::Post);

    window.with_webview(move |wv| {
        let wv = wv.inner();
        if is_post {
            // Load SAML request as HTML if POST binding is used
            info!("Loading SAML request as HTML");
            wv.load_html(&saml_request, None);
        } else {
            // Redirect to SAML request URL if REDIRECT binding is used
            info!("Redirecting to SAML request URL");
            wv.load_uri(&saml_request);
        }
    })
}

async fn show_window_after_timeout(window: Window) {
    tokio::time::sleep(Duration::from_secs(FALLBACK_SHOW_WINDOW_TIMEOUT)).await;
    info!(
        "Showing window after timeout ({:?} seconds)",
        FALLBACK_SHOW_WINDOW_TIMEOUT
    );
    show_window(&window);
}

async fn process_auth_event(
    window: &Window,
    mut event_rx: mpsc::Receiver<AuthEvent>,
) -> Option<AuthData> {
    info!("Processing auth event...");

    let (cancel_timeout_tx, cancel_timeout_rx) = mpsc::channel::<()>(1);
    let cancel_timeout_rx = Arc::new(Mutex::new(cancel_timeout_rx));

    loop {
        if let Some(auth_event) = event_rx.recv().await {
            match auth_event {
                AuthEvent::Request(auth_request) => {
                    info!("Got auth request from auth-request event, processing");
                    if let Err(err) = process_request(&window, auth_request) {
                        warn!("Error processing auth request: {}", err);
                    }
                }
                AuthEvent::Success(auth_data) => {
                    info!("Got auth data successfully, closing window");
                    close_window(window);
                    return Some(auth_data);
                }
                AuthEvent::Cancel => {
                    info!("User cancelled the authentication process, closing window");
                    close_window(window);
                    return None;
                }
                AuthEvent::Error(AuthError::TokenInvalid) => {
                    // Found the invalid token, means that user is authenticated, keep retrying and no need to show the window
                    warn!("Found invalid auth data, retrying");
                    if let Err(err) = cancel_timeout_tx.send(()).await {
                        warn!("Error sending cancel timeout: {}", err);
                    }
                    // Send the error event to the outside, so that we can retry it when receiving the auth-request event
                    if let Err(err) = window.emit_all(AUTH_ERROR_EVENT, ()) {
                        warn!("Error emitting auth-error event: {:?}", err);
                    }
                }
                AuthEvent::Error(AuthError::TokenNotFound) => {
                    let window_visible = window.is_visible().unwrap_or(false);
                    if window_visible {
                        continue;
                    }

                    info!(
                        "Token not found, showing window in {} seconds",
                        SHOW_WINDOW_TIMEOUT
                    );

                    let cancel_timeout_rx = cancel_timeout_rx.clone();
                    tokio::spawn(handle_token_not_found(window.clone(), cancel_timeout_rx));
                }
            }
        }
    }
}

/// Tokens not found means that the page might need the user interaction to login,
/// we should show the window after a short timeout, it will be cancelled if the
/// token is found in the response, no matter it's valid or not.
async fn handle_token_not_found(window: Window, cancel_timeout_rx: Arc<Mutex<mpsc::Receiver<()>>>) {
    match cancel_timeout_rx.try_lock() {
        Ok(mut cancel_timeout_rx) => {
            let duration = Duration::from_secs(SHOW_WINDOW_TIMEOUT);
            if let Err(_) = timeout(duration, cancel_timeout_rx.recv()).await {
                info!("Timeout expired, showing window");
                show_window(&window);
            } else {
                info!("Showing window timeout cancelled");
            }
        }
        Err(_) => {
            debug!("Window will be shown by another task, skipping");
        }
    }
}

/// Parse the authentication data from the response headers or HTML content
/// and send it to the event channel
fn parse_auth_data(main_res: &WebResource, event_tx: mpsc::Sender<AuthEvent>) {
    if let Some(response) = main_res.response() {
        if let Some(auth_data) = read_auth_data_from_response(&response) {
            debug!("Got auth data from HTTP headers: {:?}", auth_data);
            send_auth_data(&event_tx, auth_data);
            return;
        }
    }

    let event_tx = event_tx.clone();
    main_res.data(Cancellable::NONE, move |data| {
        if let Ok(data) = data {
            let html = String::from_utf8_lossy(&data);
            match read_auth_data_from_html(&html) {
                Ok(auth_data) => {
                    debug!("Got auth data from HTML: {:?}", auth_data);
                    send_auth_data(&event_tx, auth_data);
                }
                Err(err) => {
                    debug!("Error reading auth data from HTML: {:?}", err);
                    if let Err(err) = event_tx.blocking_send(AuthEvent::Error(err)) {
                        warn!("Error sending event: {}", err)
                    }
                }
            }
        }
    });
}

/// Read the authentication data from the response headers
fn read_auth_data_from_response(response: &webkit2gtk::URIResponse) -> Option<AuthData> {
    response.http_headers().and_then(|mut headers| {
        let auth_data = AuthData::new(
            headers.get("saml-username").map(GString::into),
            headers.get("prelogin-cookie").map(GString::into),
            headers.get("portal-userauthcookie").map(GString::into),
        );

        if auth_data.check() {
            Some(auth_data)
        } else {
            None
        }
    })
}

/// Read the authentication data from the HTML content
fn read_auth_data_from_html(html: &str) -> Result<AuthData, AuthError> {
    let saml_auth_status = parse_xml_tag(html, "saml-auth-status");

    match saml_auth_status {
        Some(status) if status == "1" => extract_auth_data(html).ok_or(AuthError::TokenInvalid),
        Some(status) if status == "-1" => Err(AuthError::TokenInvalid),
        _ => Err(AuthError::TokenNotFound),
    }
}

/// Extract the authentication data from the HTML content
fn extract_auth_data(html: &str) -> Option<AuthData> {
    let auth_data = AuthData::new(
        parse_xml_tag(html, "saml-username"),
        parse_xml_tag(html, "prelogin-cookie"),
        parse_xml_tag(html, "portal-userauthcookie"),
    );

    if auth_data.check() {
        Some(auth_data)
    } else {
        None
    }
}

fn parse_xml_tag(html: &str, tag: &str) -> Option<String> {
    let re = Regex::new(&format!("<{}>(.*)</{}>", tag, tag)).unwrap();
    re.captures(html)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

fn send_auth_data(event_tx: &mpsc::Sender<AuthEvent>, auth_data: AuthData) {
    if let Err(err) = event_tx.blocking_send(AuthEvent::Success(auth_data)) {
        warn!("Error sending event: {}", err)
    }
}

fn show_window(window: &Window) {
    let visible = window.is_visible().unwrap_or(false);
    if visible {
        debug!("Window is already visible, skipping");
        return;
    }

    if let Err(err) = window.show() {
        warn!("Error showing window: {}", err);
    }
}

fn close_window(window: &Window) {
    if let Err(err) = window.close() {
        warn!("Error closing window: {}", err);
    }
}

fn clear_webview_cookies(wv: &webkit2gtk::WebView) {
    if let Some(context) = wv.context() {
        if let Some(cookie_manager) = context.cookie_manager() {
            #[allow(deprecated)]
            cookie_manager.delete_all_cookies();
            info!("Cookies cleared");
            return;
        }
    }
    warn!("No cookie manager found");
}
