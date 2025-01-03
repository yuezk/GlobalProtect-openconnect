use objc2::{
  declare_class, msg_send_id,
  mutability::MainThreadOnly,
  rc::Retained,
  runtime::{NSObject, NSObjectProtocol},
  ClassType, DeclaredClass,
};
use objc2_foundation::{MainThreadMarker, NSHTTPURLResponse, NSURLResponse};
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKNavigationResponse, WKNavigationResponsePolicy, WKWebView};

pub struct NavigationDelegateIvars {
  pub on_response: Box<dyn Fn(Option<Retained<NSHTTPURLResponse>>) + 'static>,
}

declare_class!(
  pub struct NavigationDelegate;

  unsafe impl ClassType for NavigationDelegate {
      type Super = NSObject;
      type Mutability = MainThreadOnly;
      const NAME: &'static str = "NavigationDelegate";
  }

  impl DeclaredClass for NavigationDelegate {
      type Ivars = NavigationDelegateIvars;
  }

  unsafe impl NSObjectProtocol for NavigationDelegate {}

  unsafe impl WKNavigationDelegate for NavigationDelegate {
    #[method(webView:decidePolicyForNavigationResponse:decisionHandler:)]
    fn navigation_policy_response(
      &self,
      _wv: &WKWebView,
      response: &WKNavigationResponse,
      handler: &block2::Block<dyn Fn(WKNavigationResponsePolicy)>,
    ) {
      println!("navigation_policy_response start");

      unsafe {
        if response.isForMainFrame() {
          let url_response: Retained<NSURLResponse> = response.response();
          let http_response = Retained::cast::<NSHTTPURLResponse>(url_response);
          (self.ivars().on_response)(Some(http_response));
        }
      }

      println!("navigation_policy_response end");
      (*handler).call((WKNavigationResponsePolicy::Allow,));
    }

    #[method(webView:didFinishNavigation:)]
    fn webview_did_finish_navigation(
        &self,
        _wv: &WKWebView,
        _navigation: Option<&WKNavigation>,
    ) {
      (self.ivars().on_response)(None);
    }
  }
);

impl NavigationDelegate {
  pub fn new<F>(on_response: F) -> Retained<Self>
  where
    F: Fn(Option<Retained<NSHTTPURLResponse>>) + 'static,
  {
    let mtm = MainThreadMarker::new().expect("Not on main thread");
    let delegate = mtm.alloc::<NavigationDelegate>().set_ivars(NavigationDelegateIvars {
      on_response: Box::new(on_response),
    });

    unsafe { msg_send_id![super(delegate), init] }
  }
}
