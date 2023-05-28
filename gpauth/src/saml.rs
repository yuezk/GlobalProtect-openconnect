use regex::Regex;
use std::{cell::RefCell, rc::Rc};
use webkit2gtk::gio::Cancellable;
use webkit2gtk::glib::GString;
use webkit2gtk::{LoadEvent, URIResponseExt, WebResourceExt, WebViewExt};
use wry::application::event::{Event, StartCause, WindowEvent};
use wry::application::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use wry::application::platform::run_return::EventLoopExtRunReturn;
use wry::application::window::WindowBuilder;
use wry::webview::{WebViewBuilder, WebviewExtUnix};

#[derive(Debug, Default)]
pub(crate) struct SamlAuth {}

pub(crate) struct SamlOptions {
    binding: SamlBinding,
    target: String,
    user_agent: String,
}

impl SamlOptions {
    pub fn new(binding: SamlBinding, target: String, user_agent: String) -> Self {
        Self {
            binding,
            target,
            user_agent,
        }
    }
}

impl SamlAuth {
    pub async fn process(&self, options: SamlOptions) -> Result<(), Box<dyn std::error::Error>> {
        let saml_result = saml_login(options.binding, options.target, options.user_agent);
        println!("SAML result: {:?}", saml_result);
        Ok(())
    }
}

pub enum SamlBinding {
    Redirect,
    Post,
}

#[derive(Debug, Clone)]
pub struct SamlResult {
    username: Option<String>,
    prelogin_cookie: Option<String>,
    portal_userauthcookie: Option<String>,
}

impl SamlResult {
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

#[derive(Debug, PartialEq)]
enum SamlResultError {
    NotFound,
    Invalid,
}

#[derive(Debug)]
enum UserEvent {
    SamlSuccess(SamlResult),
    SamlError(String),
}

pub fn saml_login(
    binding: SamlBinding,
    target: String,
    user_agent: String,
) -> Result<SamlResult, Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<UserEvent> = EventLoop::with_user_event();
    let event_proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("GlobalProtect Login")
        .build(&event_loop)?;

    let wv_builder = WebViewBuilder::new(window)?.with_user_agent(&user_agent);
    let wv_builder = if let SamlBinding::Redirect = binding {
        wv_builder.with_url(&target)?
    } else {
        wv_builder.with_html(&target)?
    };

    let wv = wv_builder.build()?;
    let wv = wv.webview();

    wv.connect_load_changed(move |webview, event| {
        if let LoadEvent::Finished = event {
            if let Some(main_resource) = webview.main_resource() {
                // Read the SAML result from the HTTP headers
                if let Some(response) = main_resource.response() {
                    if let Some(saml_result) = read_saml_result_from_response(&response) {
                        println!("Got SAML result from HTTP headers");
                        return emit_event(&event_proxy, UserEvent::SamlSuccess(saml_result));
                    }
                }

                // Read the SAML result from the HTTP body
                let event_proxy = event_proxy.clone();
                main_resource.data(Cancellable::NONE, move |data| {
                    if let Ok(data) = data {
                        match read_saml_result_from_html(&data) {
                            Ok(saml_result) => {
                                println!("Got SAML result from HTTP body");
                                emit_event(&event_proxy, UserEvent::SamlSuccess(saml_result));
                            }
                            Err(err) if err == SamlResultError::Invalid => {
                                println!("Error reading SAML result from HTTP body: {:?}", err);
                                emit_event(
                                    &event_proxy,
                                    UserEvent::SamlError("Invalid SAML result".into()),
                                );
                            }
                            Err(_) => {
                                println!("SAML result not found in HTTP body");
                            }
                        }
                    }
                });
            }
        }
    });

    let saml_result: Rc<RefCell<Option<SamlResult>>> = Rc::new(RefCell::new(None));
    let saml_result_clone = saml_result.clone();

    let exit_code = event_loop.run_return(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => println!("Wry has started!"),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("User closed the window");
                *control_flow = ControlFlow::Exit
            }
            Event::UserEvent(UserEvent::SamlSuccess(result)) => {
                *saml_result_clone.borrow_mut() = Some(result);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::SamlError(_)) => {
                println!("Error reading SAML result");
                wv.load_uri("https://baidu.com");
            }
            _ => (),
        }
    });
    println!("Exit code: {:?}", exit_code);

    let saml_result = if let Some(saml_result) = saml_result.borrow().clone() {
        println!("SAML result: {:?}", saml_result);
        Ok(saml_result)
    } else {
        println!("SAML result: None");
        // TODO: Return a proper error
        Err("SAML result not found".into())
    };

    saml_result
}

fn read_saml_result_from_response(response: &webkit2gtk::URIResponse) -> Option<SamlResult> {
    response.http_headers().and_then(|mut headers| {
        let saml_result = SamlResult::new(
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

fn read_saml_result_from_html(data: &[u8]) -> Result<SamlResult, SamlResultError> {
    let body = String::from_utf8_lossy(data);
    let saml_auth_status = parse_saml_tag(&body, "saml-auth-status");

    match saml_auth_status {
        Some(status) if status == "1" => extract_saml_result(&body).ok_or(SamlResultError::Invalid),
        Some(status) if status == "-1" => Err(SamlResultError::Invalid),
        _ => Err(SamlResultError::NotFound),
    }
}

fn extract_saml_result(body: &str) -> Option<SamlResult> {
    let saml_result = SamlResult::new(
        parse_saml_tag(body, "saml-username"),
        parse_saml_tag(body, "prelogin-cookie"),
        parse_saml_tag(body, "portal-userauthcookie"),
    );

    if saml_result.check() {
        Some(saml_result)
    } else {
        None
    }
}

fn parse_saml_tag(body: &str, tag: &str) -> Option<String> {
    let re = Regex::new(&format!("<{}>(.*)</{}>", tag, tag)).unwrap();
    re.captures(body)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

fn emit_event(event_proxy: &EventLoopProxy<UserEvent>, event: UserEvent) {
    if let Err(err) = event_proxy.send_event(event) {
        println!("Error sending event: {:?}", err);
    }
}
