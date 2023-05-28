use gpauth::{AuthService, saml_login, SamlBinding};

#[tokio::main]
async fn main() {
    let url = String::from("https://globalprotect.kochind.com/global-protect/prelogin.esp?tmp=tmp&kerberos-support=yes&ipv6-support=yes&clientVer=4100&clientos=Linux");
    let _html = String::from(
        r#"<html>
    <body>
    <form id="myform" method="POST" action="https://auth.kochid.com/idp/SSO.saml2">
    <input type="hidden" name="SAMLRequest" value="PHNhbWxwOkF1dGhuUmVxdWVzdCB4bWxuczpzYW1scD0idXJuOm9hc2lzOm5hbWVzOnRjOlNBTUw6Mi4wOnByb3RvY29sIiBBc3NlcnRpb25Db25zdW1lclNlcnZpY2VVUkw9Imh0dHBzOi8vZ2xvYmFscHJvdGVjdC5rb2NoaW5kLmNvbTo0NDMvU0FNTDIwL1NQL0FDUyIgRGVzdGluYXRpb249Imh0dHBzOi8vYXV0aC5rb2NoaWQuY29tL2lkcC9TU08uc2FtbDIiIElEPSJfZmEzZTA4NDE5NjdkZTdlYzUyNzc4Nzc4YzBkOTViMDEiIElzc3VlSW5zdGFudD0iMjAyMy0wNS0yNFQwNToyNDo1OVoiIFByb3RvY29sQmluZGluZz0idXJuOm9hc2lzOm5hbWVzOnRjOlNBTUw6Mi4wOmJpbmRpbmdzOkhUVFAtUE9TVCIgVmVyc2lvbj0iMi4wIj48c2FtbDpJc3N1ZXIgeG1sbnM6c2FtbD0idXJuOm9hc2lzOm5hbWVzOnRjOlNBTUw6Mi4wOmFzc2VydGlvbiI+aHR0cHM6Ly9nbG9iYWxwcm90ZWN0LmtvY2hpbmQuY29tOjQ0My9TQU1MMjAvU1A8L3NhbWw6SXNzdWVyPjwvc2FtbHA6QXV0aG5SZXF1ZXN0Pg==" />
    <input type="hidden" name="RelayState" value="rgbNAP1wSGI0NGE1ZDZjOGM4YTkzNjk5NWNhY2JlZjkwMWJmMzIwYg==" />
    </form>
    <script>
      document.getElementById('myform').subm{
    let (client, server) = duplex(1);
    AuthService { client, server }
}>
    </body>
    </html>
    "#,
    );

    let ua = String::from("PAN GlobalProtect");
    match saml_login(SamlBinding::Redirect, url, ua) {
        Ok(saml_result) => {
            println!("SAML result: {:?}", saml_result);
        }
        Err(err) => {
            println!("Error: {:?}", err);
        }
    }

    // let mut auth_service = AuthService::default();
    // let client = auth_service.client();

    // tokio::spawn(async move {
    //     let mut client = client.lock().await;
    //     client.write("Hello").await.expect("write failed");

    //     loop {
    //         if let Ok(data) = client.read().await {
    //             println!("Received: {}", data);
    //         }
    //     }
    // });

    // tokio::select! {
    //     _ = auth_service.run() => {
    //         println!("AuthService exited");
    //     }
    //     _ = tokio::signal::ctrl_c() => {
    //         println!("Ctrl-C received, exiting");
    //     }
    // }
}
