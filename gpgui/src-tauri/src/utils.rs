use log::{info, warn};
use std::{path::PathBuf, time::Instant};
use tauri::{AppHandle, Window};
use tokio::sync::oneshot;
use url::{form_urlencoded, Url};
use webkit2gtk::{
    gio::Cancellable, glib::TimeSpan, WebContextExt, WebViewExt, WebsiteDataManagerExtManual,
    WebsiteDataTypes,
};

pub(crate) fn redact_url(url: &str) -> String {
    if let Ok(mut url) = Url::parse(url) {
        if let Err(err) = url.set_host(Some("redacted")) {
            warn!("Error redacting URL: {}", err);
        }

        let query = url.query().unwrap_or_default();
        if !query.is_empty() {
            // Replace the query value with <redacted> for each key.
            let redacted_query = redact_query(url.query().unwrap_or(""));
            url.set_query(Some(&redacted_query));
        }
        url.to_string()
    } else {
        warn!("Error parsing URL: {}", url);
        url.to_string()
    }
}

fn redact_query(query: &str) -> String {
    let query_pairs = form_urlencoded::parse(query.as_bytes());
    let mut redacted_pairs = query_pairs.map(|(key, _)| (key, "__redacted__"));

    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(redacted_pairs.by_ref())
        .finish()
}

pub(crate) async fn clear_webview_cookies(window: &Window) -> Result<(), tauri::Error> {
    let (tx, rx) = oneshot::channel::<()>();

    window.with_webview(|wv| {
        let wv = wv.inner();
        let context = match wv.context() {
            Some(context) => context,
            None => {
                return send_error(tx, "No context found");
            }
        };
        let data_manager = match context.website_data_manager() {
            Some(manager) => manager,
            None => {
                return send_error(tx, "No data manager found");
            }
        };

        let now = Instant::now();
        data_manager.clear(
            WebsiteDataTypes::COOKIES,
            TimeSpan(0),
            Cancellable::NONE,
            move |result| match result {
                Err(err) => {
                    send_error(tx, &err.to_string());
                }
                Ok(_) => {
                    info!("Cookies cleared in {} ms", now.elapsed().as_millis());
                    send_result(tx);
                }
            },
        );
    })?;

    rx.await.map_err(|_| tauri::Error::FailedToSendMessage)
}

fn send_error(tx: oneshot::Sender<()>, message: &str) {
    warn!("Error clearing cookies: {}", message);
    if tx.send(()).is_err() {
        warn!("Error sending clear cookies result");
    }
}

fn send_result(tx: oneshot::Sender<()>) {
    if tx.send(()).is_err() {
        warn!("Error sending clear cookies result");
    }
}

pub(crate) fn get_openssl_conf() -> String {
    // OpenSSL version number format: 0xMNN00PP0L
    // https://www.openssl.org/docs/man3.0/man3/OPENSSL_VERSION_NUMBER.html
    let version_3_0_4: i64 = 0x30000040;
    let openssl_version = openssl::version::number();

    // See: https://stackoverflow.com/questions/75763525/curl-35-error0a000152ssl-routinesunsafe-legacy-renegotiation-disabled
    let option = if openssl_version >= version_3_0_4 {
        "UnsafeLegacyServerConnect"
    } else {
        "UnsafeLegacyRenegotiation"
    };

    format!(
        "openssl_conf = openssl_init

[openssl_init]
ssl_conf = ssl_sect

[ssl_sect]
system_default = system_default_sect

[system_default_sect]
Options = {}",
        option
    )
}

pub(crate) fn get_openssl_conf_path(app_handle: &AppHandle) -> PathBuf {
    let app_dir = app_handle
        .path_resolver()
        .app_data_dir()
        .expect("failed to resolve app dir");

    app_dir.join("openssl.cnf")
}
