use webkit2gtk::LoadEvent;
use webkit2gtk::URIResponseExt;
use webkit2gtk::WebResourceExt;
use webkit2gtk::WebViewExt;
use wry::application::event::Event;
use wry::application::event::StartCause;
use wry::application::event::WindowEvent;
use wry::application::event_loop::ControlFlow;
use wry::application::event_loop::EventLoop;
use wry::application::window::WindowBuilder;
use wry::webview::WebViewBuilder;
use wry::webview::WebviewExtUnix;

fn main() -> wry::Result<()> {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Hello World")
        .build(&event_loop)?;
    let _webview = WebViewBuilder::new(window)?
        .with_url("https://tauri.studio")?
        .build()?;

    let wv = _webview.webview();
    wv.connect_load_changed(|wv, load_event| {
        if load_event == LoadEvent::Finished {
            let response = wv.main_resource().unwrap().response().unwrap();
            response.http_headers().unwrap().foreach(|k, v| {
                println!("{}: {}", k, v);
            });
        }
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => println!("Wry has started!"),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => (),
        }
    });
}
