use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc};
use tauri::{AppHandle, Manager, WindowBuilder, WindowEvent::CloseRequested, WindowUrl};
use url::Url;
use webkit2gtk::{
    gio::Cancellable, glib::GString, traits::WebViewExt, LoadEvent, URIResponseExt, WebResource,
    WebResourceExt,
};

const AUTH_WINDOW_LABEL: &str = "auth_window";
const AUTH_SUCCESS_EVENT: &str = "auth-success";
const AUTH_ERROR_EVENT: &str = "auth-error";
const AUTH_CANCEL_EVENT: &str = "auth-cancel";
const AUTH_REQUEST_EVENT: &str = "auth-request";

#[derive(Debug, Deserialize)]
pub(crate) enum SamlBinding {
    #[serde(rename = "REDIRECT")]
    Redirect,
    #[serde(rename = "POST")]
    Post,
}

pub(crate) struct AuthOptions {
    saml_binding: SamlBinding,
    saml_request: String,
    user_agent: String,
}

#[derive(Debug, Deserialize)]
struct AuthRequestPayload {
    #[serde(alias = "samlRequest")]
    saml_request: String,
}

impl AuthOptions {
    pub fn new(saml_binding: SamlBinding, saml_request: String, user_agent: String) -> Self {
        Self {
            saml_binding,
            saml_request,
            user_agent,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthData {
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
    NotFound,
    Invalid,
}

#[derive(Debug)]
struct AuthEventEmitter {
    app_handle: AppHandle,
}

impl AuthEventEmitter {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    fn emit_success(&self, saml_result: AuthData) {
        self.app_handle.emit_all(AUTH_SUCCESS_EVENT, saml_result);
        if let Some(window) = self.app_handle.get_window(AUTH_WINDOW_LABEL) {
            window.close();
        }
    }

    fn emit_error(&self, error: String) {
        self.app_handle.emit_all(AUTH_ERROR_EVENT, error);
    }

    fn emit_cancel(&self) {
        self.app_handle.emit_all(AUTH_CANCEL_EVENT, ());
    }
}

#[derive(Debug)]
pub(crate) struct AuthWindow {
    event_emitter: Arc<AuthEventEmitter>,
    app_handle: AppHandle,
    saml_binding: SamlBinding,
    user_agent: String,
}

impl AuthWindow {
    pub fn new(app_handle: AppHandle, saml_binding: SamlBinding, user_agent: String) -> Self {
        Self {
            event_emitter: Arc::new(AuthEventEmitter::new(app_handle.clone())),
            app_handle,
            saml_binding,
            user_agent,
        }
    }

    pub fn process(&self, saml_request: String) -> tauri::Result<()> {
        let url = self.window_url(&saml_request)?;
        let window = WindowBuilder::new(&self.app_handle, AUTH_WINDOW_LABEL, url)
            .title("GlobalProtect Login")
            .user_agent(&self.user_agent)
            .always_on_top(true)
            .focused(true)
            .center()
            .build()?;

        let event_emitter = self.event_emitter.clone();
        let is_post = matches!(self.saml_binding, SamlBinding::Post);

        window.with_webview(move |wv| {
            let wv = wv.inner();
            // Load SAML request as HTML if POST binding is used
            if is_post {
                wv.load_html(&saml_request, None);
            }
            wv.connect_load_changed(move |wv, event| {
                if LoadEvent::Finished == event {
                    if let Some(uri) = wv.uri() {
                        if uri.is_empty() {
                            println!("Empty URI");
                            event_emitter.emit_error("Empty URI".to_string());
                            return;
                        } else {
                            println!("Loaded URI: {}", uri);
                        }
                    }

                    if let Some(main_res) = wv.main_resource() {
                        AuthResultParser::new(&event_emitter).parse(&main_res);
                    }
                }
            });
        })?;

        let event_emitter = self.event_emitter.clone();
        window.on_window_event(move |event| {
            if let CloseRequested { .. } = event {
                event_emitter.emit_cancel();
            }
        });

        let window_clone = window.clone();
        window.listen_global(AUTH_REQUEST_EVENT, move |event| {
            let auth_request_payload: AuthRequestPayload = serde_json::from_str(event.payload().unwrap()).unwrap();
            let saml_request = auth_request_payload.saml_request;

            window_clone.with_webview(move |wv| {
                let wv = wv.inner();
                if is_post {
                    // Load SAML request as HTML if POST binding is used
                    wv.load_html(&saml_request, None);
                } else {
                    println!("Redirecting to SAML request URL: {}", saml_request);
                    // Redirect to SAML request URL if REDIRECT binding is used
                    wv.load_uri(&saml_request);
                }
            });
        });

        Ok(())
    }

    fn window_url(&self, saml_request: &String) -> tauri::Result<WindowUrl> {
        match self.saml_binding {
            SamlBinding::Redirect => match Url::parse(saml_request) {
                Ok(url) => Ok(WindowUrl::External(url)),
                Err(err) => Err(tauri::Error::InvalidUrl(err)),
            },
            SamlBinding::Post => Ok(WindowUrl::App("auth.html".into())),
        }
    }
}

struct AuthResultParser<'a> {
    event_emitter: &'a Arc<AuthEventEmitter>,
}

impl<'a> AuthResultParser<'a> {
    fn new(event_emitter: &'a Arc<AuthEventEmitter>) -> Self {
        Self { event_emitter }
    }

    fn parse(&self, main_res: &WebResource) {
        if let Some(response) = main_res.response() {
            if let Some(saml_result) = read_auth_result_from_response(&response) {
                // Got SAML result from HTTP headers
                println!("SAML result: {:?}", saml_result);
                self.event_emitter.emit_success(saml_result);
                return;
            }
        }

        let event_emitter = self.event_emitter.clone();
        main_res.data(Cancellable::NONE, move |data| {
            if let Ok(data) = data {
                let html = String::from_utf8_lossy(&data);
                match read_auth_result_from_html(&html) {
                    Ok(saml_result) => {
                        // Got SAML result from HTML
                        println!("SAML result: {:?}", saml_result);
                        event_emitter.emit_success(saml_result);
                        return;
                    }
                    Err(AuthError::Invalid) => {
                        // Invalid SAML result
                        println!("Invalid SAML result");
                        event_emitter.emit_error("Invalid SAML result".to_string())
                    }
                    Err(AuthError::NotFound) => {
                        let has_form = html.contains("</form>");
                        if has_form {
                            // SAML form found
                            println!("SAML form found");
                        } else {
                            // No SAML form found
                            println!("No SAML form found");
                        }
                    },
                }
            }
        });
    }
}

fn read_auth_result_from_response(response: &webkit2gtk::URIResponse) -> Option<AuthData> {
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

fn read_auth_result_from_html(html: &str) -> Result<AuthData, AuthError> {
    let saml_auth_status = parse_xml_tag(html, "saml-auth-status");
    

    match saml_auth_status {
        Some(status) if status == "1" => extract_auth_data(html).ok_or(AuthError::Invalid),
        Some(status) if status == "-1" => Err(AuthError::Invalid),
        _ => Err(AuthError::NotFound),
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
