use block2::RcBlock;
use objc2::{
  declare_class, msg_send_id,
  mutability::MainThreadOnly,
  rc::Retained,
  runtime::{AnyObject, NSObject, NSObjectProtocol},
  ClassType, DeclaredClass,
};
use objc2_foundation::{MainThreadMarker, NSError, NSHTTPURLResponse, NSString, NSURLResponse};
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKNavigationResponse, WKNavigationResponsePolicy, WKWebView};

pub struct NavigationDelegateIvars {
  pub on_response: Box<dyn Fn(Retained<NSHTTPURLResponse>) + 'static>,
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
    // #[method(webView:decidePolicyForNavigationResponse:decisionHandler:)]
    // fn navigation_policy_response(
    //   &self,
    //   wv: &WKWebView,
    //   response: &WKNavigationResponse,
    //   handler: &block2::Block<dyn Fn(WKNavigationResponsePolicy)>,
    // ) {
    //   println!("navigation_policy_response start");

    //   unsafe {
    //     if response.isForMainFrame() {
    //       let url_response: Retained<NSURLResponse> = response.response();
    //       let http_response = Retained::cast::<NSHTTPURLResponse>(url_response);
    //       // (self.ivars().on_response)(http_response);
    //     }
    //   }

    //   println!("navigation_policy_response end");
    //   (*handler).call((WKNavigationResponsePolicy::Allow,));
    // }

    #[method(webView:didFinishNavigation:)]
    fn webview_did_finish_navigation(
        &self,
        wv: &WKWebView,
        navigation: Option<&WKNavigation>,
    ) {
      println!("webView_didFinishNavigation");
      unsafe {
        let cb = RcBlock::new(|body: *mut AnyObject, err: *mut NSError| {
          let body = body as *mut NSString;
          let body = body.as_ref().unwrap();

          // let html = NSString::from_ptr(body);

          println!("html: {:?}", body);
          // NSString::from_ptr(body).map(|s| println!("html: {:?}", s));
          // println!("html: {:?}", body);
        });

        wv.evaluateJavaScript_completionHandler(
          &NSString::from_str("document.documentElement.outerHTML"),
          Some(&cb)
        );
      }
    }
  }
);

impl NavigationDelegate {
  pub fn new<F>(on_response: F) -> Retained<Self>
  where
    F: Fn(Retained<NSHTTPURLResponse>) + 'static,
  {
    let mtm = MainThreadMarker::new().expect("Not on main thread");
    let delegate = mtm.alloc::<NavigationDelegate>().set_ivars(NavigationDelegateIvars {
      on_response: Box::new(on_response),
    });

    unsafe { msg_send_id![super(delegate), init] }
  }
}
