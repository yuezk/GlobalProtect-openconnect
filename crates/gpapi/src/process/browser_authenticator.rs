use std::{
  borrow::Cow,
  env::temp_dir,
  fs,
  io::{Read, Write},
  net::{TcpListener, UdpSocket},
  os::unix::fs::PermissionsExt,
  thread,
};

use anyhow::bail;
use log::{info, warn};

use crate::auth::SamlAuthData;

const REMOTE_AUTH_PATH: &str = "/auth";

pub struct BrowserAuthenticator<'a> {
  auth_request: &'a str,
  browser: Option<&'a str>,
}

impl BrowserAuthenticator<'_> {
  pub fn new(auth_request: &str) -> BrowserAuthenticator {
    BrowserAuthenticator {
      auth_request,
      browser: None,
    }
  }

  pub fn new_with_browser<'a>(auth_request: &'a str, browser: &'a str) -> BrowserAuthenticator<'a> {
    let browser = browser.trim();
    BrowserAuthenticator {
      auth_request,
      browser: if browser.is_empty() || browser == "default" {
        None
      } else {
        Some(browser)
      },
    }
  }

  pub fn authenticate(&self) -> anyhow::Result<Option<SamlAuthData>> {
    if self
      .browser
      .is_some_and(|browser| browser.eq_ignore_ascii_case("remote"))
    {
      return self.authenticate_remote();
    }

    let path = if self.auth_request.starts_with("http") {
      Cow::Borrowed(self.auth_request)
    } else {
      let html_file = temp_dir().join("gpauth.html");

      // Remove the file and error if permission denied
      if let Err(err) = fs::remove_file(&html_file) {
        if err.kind() != std::io::ErrorKind::NotFound {
          warn!("Failed to remove the temporary file: {}", err);
          bail!("Please remove the file manually: {:?}", html_file);
        }
      }

      let mut file = fs::File::create(&html_file)?;

      file.set_permissions(fs::Permissions::from_mode(0o600))?;
      file.write_all(self.auth_request.as_bytes())?;

      Cow::Owned(html_file.to_string_lossy().to_string())
    };

    if let Some(browser) = self.browser {
      let app = find_browser_path(browser);

      info!("Launching browser: {}", app);
      open::with_detached(path.as_ref(), app)?;
    } else {
      info!("Launching the default browser...");
      webbrowser::open(path.as_ref())?;
    }

    Ok(None)
  }

  fn authenticate_remote(&self) -> anyhow::Result<Option<SamlAuthData>> {
    let local_ip = detect_local_ip()?;
    let listener = TcpListener::bind((local_ip.as_str(), 0))?;
    let auth_url = format!("http://{}{}", listener.local_addr()?, REMOTE_AUTH_PATH);
    let auth_request = self.auth_request.to_string();

    thread::spawn(move || serve_auth_request(listener, auth_request));

    info!(
      r#"

==== Manual Authentication Required ====

Please open the following URL in your browser:

{}

After completing the authentication, please paste the authentication data back to this terminal.
(The data should start with "globalprotectcallback:...")
"#,
      auth_url
    );

    let mut data = String::new();
    std::io::stdin().read_line(&mut data)?;

    Ok(Some(SamlAuthData::from_gpcallback(data.trim())?))
  }
}

fn find_browser_path(browser: &str) -> String {
  if browser == "chrome" {
    which::which("google-chrome-stable")
      .or_else(|_| which::which("google-chrome"))
      .or_else(|_| which::which("chromium"))
      .map(|path| path.to_string_lossy().to_string())
      .unwrap_or_else(|_| browser.to_string())
  } else {
    browser.into()
  }
}

fn detect_local_ip() -> anyhow::Result<String> {
  let socket = UdpSocket::bind("0.0.0.0:0")?;
  socket.connect("1.1.1.1:80")?;

  let ip = socket.local_addr()?.ip().to_string();
  info!("Determined local IP address: {}", ip);

  Ok(ip)
}

fn serve_auth_request(listener: TcpListener, auth_request: String) {
  let Ok(addr) = listener.local_addr() else {
    warn!("Failed to determine auth server address");
    return;
  };

  let auth_url = format!("http://{}{}", addr, REMOTE_AUTH_PATH);
  info!("Auth server started at: {}", auth_url);

  for incoming in listener.incoming() {
    let mut stream = match incoming {
      Ok(stream) => stream,
      Err(err) => {
        warn!("Failed to accept auth request: {}", err);
        continue;
      }
    };

    let mut request = [0_u8; 4096];
    let size = match stream.read(&mut request) {
      Ok(size) => size,
      Err(err) => {
        warn!("Failed to read auth request: {}", err);
        continue;
      }
    };

    let request = String::from_utf8_lossy(&request[..size]);
    let path = request
      .lines()
      .next()
      .and_then(|line| line.split_whitespace().nth(1))
      .unwrap_or("/");

    if path != REMOTE_AUTH_PATH {
      let _ = write_response(&mut stream, "404 Not Found", "text/plain", "not found", None);
      continue;
    }

    let response = if auth_request.starts_with("http") {
      write_response(
        &mut stream,
        "302 Found",
        "text/plain",
        "redirect",
        Some(("Location", auth_request.as_str())),
      )
    } else {
      write_response(&mut stream, "200 OK", "text/html; charset=utf-8", &auth_request, None)
    };

    if let Err(err) = response {
      warn!("Failed to write auth response: {}", err);
    }

    break;
  }
}

fn write_response(
  stream: &mut std::net::TcpStream,
  status: &str,
  content_type: &str,
  body: &str,
  extra_header: Option<(&str, &str)>,
) -> std::io::Result<()> {
  write!(stream, "HTTP/1.1 {}\r\n", status)?;
  write!(stream, "Content-Type: {}\r\n", content_type)?;
  if let Some((name, value)) = extra_header {
    write!(stream, "{}: {}\r\n", name, value)?;
  }
  write!(stream, "Content-Length: {}\r\n", body.len())?;
  write!(stream, "Connection: close\r\n\r\n")?;
  stream.write_all(body.as_bytes())?;
  stream.flush()
}
