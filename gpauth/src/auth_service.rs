use crate::{
    duplex::duplex,
    saml::{SamlAuth, SamlBinding, SamlOptions},
    DuplexStreamHandle,
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct AuthService {
    server: DuplexStreamHandle,
    client: Arc<Mutex<DuplexStreamHandle>>,
    saml_auth: Arc<SamlAuth>,
}

impl Default for AuthService {
    fn default() -> Self {
        let (client, server) = duplex(4096);
        Self {
            client: Arc::new(Mutex::new(client)),
            server,
            saml_auth: Default::default(),
        }
    }
}

impl AuthService {
    pub async fn run(&mut self) {
        loop {
            println!("Server waiting for data");
            match self.server.read().await {
                Ok(data) => {
                    println!("Server received: {}", data);
                    let target = String::from("https://login.microsoftonline.com/901c038b-4638-4259-b115-c1753c7735aa/saml2?SAMLRequest=lVLBbsIwDP2VKveSNGlaiGilDg5DYlpFux12mdIQIFKbdEmKtr8fhaGxC9Lkk%2BXnZ79nzx3v2p4Vgz%2FojfwYpPPBZ9dqx86FDAxWM8OdckzzTjrmBauKpzXDE8R6a7wRpgVB4Zy0Xhm9MNoNnbSVtEcl5MtmnYGD971jEB57PemUsMZ5y73cf02E6VgcEzgyYgSrEhaLCgTL0xZK85Hvt7s1e3XtNztvdKu0HBngDEUCkWkTxgmZhjGms7CJIhqKKKVEpCmhnMNRDgbBapmBdzpL5BZFEu0oalKMpglttukpaBLHuEEnmHODXGnnufYZwAiTENEQ0xoljBJGyBsIyh%2F1D0pvld7ft6q5gBx7rOsyLJ%2BrGgSv0rqzxBMA5PNxQ3YebG9OcJ%2BWX30H%2BT9cnsObWfkl%2B%2FsD%2BTc%3D&RelayState=HEgCAOLrNmRmZTBkM2FlNDE2MDQyMDhjZTVmMTZlMTdiZTdiMTliNg%3D%3D");
                    let ua = String::from("PAN GlobalProtect");

                    let saml_options = SamlOptions::new(SamlBinding::Redirect, target, ua);
                    let saml_auth = self.saml_auth.clone();
                    tokio::spawn(async move {
                        saml_auth.process(saml_options).await;
                    });
                    // self.server.write(&data).await.expect("write failed");
                }
                Err(err) => {
                    println!("Server error: {:?}", err);
                }
            }
        }
    }

    pub fn client(&self) -> Arc<Mutex<DuplexStreamHandle>> {
        self.client.clone()
    }
}
