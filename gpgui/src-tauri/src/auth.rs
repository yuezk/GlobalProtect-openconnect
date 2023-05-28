use log::{debug, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tauri::EventHandler;
use tauri::{AppHandle, Manager, Window, WindowBuilder, WindowEvent::CloseRequested, WindowUrl};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use webkit2gtk::{
    gio::Cancellable, glib::GString, traits::WebViewExt, LoadEvent, URIResponseExt, WebResource,
    WebResourceExt,
};

const AUTH_WINDOW_LABEL: &str = "auth_window";
const AUTH_ERROR_EVENT: &str = "auth-error";
const AUTH_REQUEST_EVENT: &str = "auth-request";

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
    app_handle: &AppHandle,
) -> tauri::Result<Option<AuthData>> {
    let (event_tx, event_rx) = mpsc::channel::<AuthEvent>(8);
    let window = build_window(app_handle, ua)?;
    setup_webview(&window, event_tx.clone())?;
    let handler_id = setup_window(&window, event_tx);

    match process(&window, event_rx, auth_request).await {
        Ok(auth_data) => {
            window.unlisten(handler_id);
            Ok(auth_data)
        }
        Err(err) => {
            window.unlisten(handler_id);
            Err(err)
        }
    }
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

fn setup_webview(window: &Window, event_tx: mpsc::Sender<AuthEvent>) -> tauri::Result<()> {
    window.with_webview(move |wv| {
        let wv = wv.inner();
        let event_tx = event_tx.clone();

        wv.connect_load_changed(move |wv, event| {
            if LoadEvent::Finished != event {
                debug!("Skipping load event: {:?}", event);
                return;
            }

            let uri = wv.uri().unwrap_or("".into());
            // Empty URI indicates that an error occurred
            if uri.is_empty() {
                warn!("Empty URI");
                if let Err(err) = event_tx.blocking_send(AuthEvent::Error(AuthError::TokenInvalid))
                {
                    println!("Error sending event: {}", err);
                }
                return;
            }
            // TODO, redact URI
            debug!("Loaded URI: {}", uri);

            if let Some(main_res) = wv.main_resource() {
                // AuthDataParser::new(&window_tx_clone).parse(&main_res);
                parse_auth_data(&main_res, event_tx.clone());
            } else {
                warn!("No main_resource");
            }
        });

        wv.connect_load_failed(|_wv, event, err_msg, err| {
            println!("Load failed: {:?}, {}, {:?}", event, err_msg, err);
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
                println!("Error sending event: {}", err)
            }
        }
    });

    window.open_devtools();

    window.listen_global(AUTH_REQUEST_EVENT, move |event| {
        if let Ok(payload) = TryInto::<AuthRequest>::try_into(event.payload()) {
            debug!("---------Received auth request");

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
    event_rx: mpsc::Receiver<AuthEvent>,
    auth_request: AuthRequest,
) -> tauri::Result<Option<AuthData>> {
    process_request(window, auth_request)?;

    let (close_tx, close_rx) = mpsc::channel::<()>(1);

    tokio::spawn(show_window_after_timeout(window.clone(), close_rx));
    process_auth_event(&window, event_rx, close_tx).await
}

fn process_request(window: &Window, auth_request: AuthRequest) -> tauri::Result<()> {
    let saml_request = auth_request.saml_request;
    let is_post = matches!(auth_request.saml_binding, SamlBinding::Post);

    window.with_webview(move |wv| {
        let wv = wv.inner();
        if is_post {
            // Load SAML request as HTML if POST binding is used
            wv.load_html(&saml_request, None);
        } else {
            println!("Redirecting to SAML request URL: {}", saml_request);
            // Redirect to SAML request URL if REDIRECT binding is used
            wv.load_uri(&saml_request);
        }
    })
}

async fn show_window_after_timeout(window: Window, mut close_rx: mpsc::Receiver<()>) {
    // Show the window after 10 seconds
    let duration = Duration::from_secs(10);
    if let Err(_) = timeout(duration, close_rx.recv()).await {
        println!("Final show window");
        show_window(&window);
    } else {
        println!("Window closed, cancel the final show window");
    }
}

async fn process_auth_event(
    window: &Window,
    mut event_rx: mpsc::Receiver<AuthEvent>,
    close_tx: mpsc::Sender<()>,
) -> tauri::Result<Option<AuthData>> {
    let (cancel_timeout_tx, cancel_timeout_rx) = mpsc::channel::<()>(1);
    let cancel_timeout_rx = Arc::new(Mutex::new(cancel_timeout_rx));

    async fn close_window(window: &Window, close_tx: mpsc::Sender<()>) {
        if let Err(err) = window.close() {
            println!("Error closing window: {}", err);
        }
        if let Err(err) = close_tx.send(()).await {
            warn!("Error sending the close event: {:?}", err);
        }
    }

    loop {
        if let Some(auth_event) = event_rx.recv().await {
            match auth_event {
                AuthEvent::Request(auth_request) => {
                    println!("Got auth request: {:?}", auth_request);
                    process_request(&window, auth_request)?;
                }
                AuthEvent::Success(auth_data) => {
                    close_window(window, close_tx).await;
                    return Ok(Some(auth_data));
                }
                AuthEvent::Cancel => {
                    close_window(window, close_tx).await;
                    return Ok(None);
                }
                AuthEvent::Error(AuthError::TokenInvalid) => {
                    if let Err(err) = cancel_timeout_tx.send(()).await {
                        println!("Error sending event: {}", err);
                    }
                    if let Err(err) =
                        window.emit_all(AUTH_ERROR_EVENT, "Invalid SAML result".to_string())
                    {
                        warn!("Error emitting auth-error event: {:?}", err);
                    }
                }
                AuthEvent::Error(AuthError::TokenNotFound) => {
                    let cancel_timeout_rx = cancel_timeout_rx.clone();
                    tokio::spawn(handle_token_not_found(window.clone(), cancel_timeout_rx));
                }
            }
        }
    }
}

async fn handle_token_not_found(window: Window, cancel_timeout_rx: Arc<Mutex<mpsc::Receiver<()>>>) {
    // Tokens not found, show the window in 5 seconds
    match cancel_timeout_rx.try_lock() {
        Ok(mut cancel_timeout_rx) => {
            println!("Scheduling timeout");
            let duration = Duration::from_secs(5);
            if let Err(_) = timeout(duration, cancel_timeout_rx.recv()).await {
                println!("Show window after timeout");
                show_window(&window);
            } else {
                println!("Cancel timeout");
            }
        }
        Err(_) => {
            println!("Timeout already scheduled");
        }
    }
}

fn parse_auth_data(main_res: &WebResource, event_tx: mpsc::Sender<AuthEvent>) {
    if let Some(response) = main_res.response() {
        if let Some(saml_result) = read_auth_data_from_response(&response) {
            // Got SAML result from HTTP headers
            println!("SAML result: {:?}", saml_result);
            send_auth_data(&event_tx, saml_result);
            return;
        }
    }

    let event_tx = event_tx.clone();
    main_res.data(Cancellable::NONE, move |data| {
        if let Ok(data) = data {
            let html = String::from_utf8_lossy(&data);
            match read_auth_data_from_html(&html) {
                Ok(saml_result) => {
                    // Got SAML result from HTML
                    println!("SAML result: {:?}", saml_result);
                    send_auth_data(&event_tx, saml_result);
                }
                Err(err) => {
                    println!("Auth error: {:?}", err);
                    if let Err(err) = event_tx.blocking_send(AuthEvent::Error(err)) {
                        println!("Error sending event: {}", err)
                    }
                }
            }
        }
    });
}

fn read_auth_data_from_response(response: &webkit2gtk::URIResponse) -> Option<AuthData> {
    response.http_headers().and_then(|mut headers| {
        let saml_result = AuthData::new(
            headers.get("saml-username").map(GString::into),
            headers.get("prelogin-cookie").map(GString::into),
            headers.get("portal-userauthcookie").map(GString::into),
        );

        if saml_result.check() {
            Some(saml_result)
        } else {
            None
        }
    })
}

fn read_auth_data_from_html(html: &str) -> Result<AuthData, AuthError> {
    let saml_auth_status = parse_xml_tag(html, "saml-auth-status");

    match saml_auth_status {
        Some(status) if status == "1" => extract_auth_data(html).ok_or(AuthError::TokenInvalid),
        Some(status) if status == "-1" => Err(AuthError::TokenInvalid),
        _ => Err(AuthError::TokenNotFound),
    }
}

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

fn send_auth_data(event_tx: &mpsc::Sender<AuthEvent>, saml_result: AuthData) {
    if let Err(err) = event_tx.blocking_send(AuthEvent::Success(saml_result)) {
        println!("Error sending event: {}", err)
    }
}

fn show_window(window: &Window) {
    match window.is_visible() {
        Ok(true) => {
            println!("Window is already visible");
        }
        _ => {
            if let Err(err) = window.show() {
                println!("Error showing window: {}", err);
            }
        }
    }
}
