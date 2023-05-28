use log::{warn, info, debug};
use serde::{Deserialize, Serialize};
use std::ffi::{c_void, CString};
use std::sync::Arc;
use std::thread;
use tokio::sync::watch;
use tokio::sync::{mpsc, Mutex};

mod ffi;

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VpnStatus {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

#[derive(Debug)]
struct StatusHolder {
    status: VpnStatus,
    status_tx: watch::Sender<VpnStatus>,
    status_rx: watch::Receiver<VpnStatus>,
}

impl Default for StatusHolder {
    fn default() -> Self {
        let (status_tx, status_rx) = watch::channel(VpnStatus::Disconnected);

        Self {
            status: VpnStatus::Disconnected,
            status_tx,
            status_rx,
        }
    }
}

impl StatusHolder {
    fn set(&mut self, status: VpnStatus) {
        self.status = status;
        if let Err(err) = self.status_tx.send(status) {
            warn!("Failed to send VPN status: {}", err);
        }
    }

    fn status_rx(&self) -> watch::Receiver<VpnStatus> {
        self.status_rx.clone()
    }
}

#[derive(Debug)]
pub(crate) struct VpnOptions {
    server: CString,
    cookie: CString,
    script: CString,
}

impl VpnOptions {
    fn as_oc_options(&self, user_data: *mut c_void) -> ffi::Options {
        ffi::Options {
            server: self.server.as_ptr(),
            cookie: self.cookie.as_ptr(),
            script: self.script.as_ptr(),
            user_data,
        }
    }

    fn to_cstr(value: &str) -> CString {
        CString::new(value.to_string()).expect("Failed to convert to CString")
    }
}

#[derive(Debug, Default)]
pub(crate) struct Vpn {
    status_holder: Arc<Mutex<StatusHolder>>,
    vpn_options: Arc<Mutex<Option<VpnOptions>>>,
}

impl Vpn {
    pub async fn status_rx(&self) -> watch::Receiver<VpnStatus> {
        self.status_holder.lock().await.status_rx()
    }

    pub async fn connect(
        &self,
        server: &str,
        cookie: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Save the VPN options so we can use them later, e.g. reconnect
        *self.vpn_options.lock().await = Some(VpnOptions {
            server: VpnOptions::to_cstr(server),
            cookie: VpnOptions::to_cstr(cookie),
            script: VpnOptions::to_cstr("/usr/share/vpnc-scripts/vpnc-script")
        });

        let vpn_options = self.vpn_options.clone();
        let status_holder = self.status_holder.clone();
        let (vpn_tx, mut vpn_rx) = mpsc::channel::<i32>(1);

        thread::spawn(move || {
            let vpn_tx = &vpn_tx as *const _ as *mut c_void;
            let oc_options = vpn_options
                .blocking_lock()
                .as_ref()
                .expect("Failed to unwrap vpn_options")
                .as_oc_options(vpn_tx);

            // Start the VPN connection, this will block until the connection is closed
            status_holder.blocking_lock().set(VpnStatus::Connecting);
            let ret = unsafe { ffi::connect(&oc_options) };

            info!("VPN connection closed with code: {}", ret);
            status_holder.blocking_lock().set(VpnStatus::Disconnected);
        });

        info!("Waiting for the VPN connection...");

        if let Some(cmd_pipe_fd) = vpn_rx.recv().await {
            info!("VPN connection started, cmd_pipe_fd: {}", cmd_pipe_fd);
            self.status_holder.lock().await.set(VpnStatus::Connected);
        } else {
            warn!("VPN connection failed to start");
        }

        Ok(())
    }

    pub async fn disconnect(&self) {
        if self.status().await == VpnStatus::Disconnected {
            info!("VPN is not connected, nothing to do");
            return;
        }

        info!("Disconnecting VPN...");
        unsafe { ffi::disconnect() };

        let mut status_rx = self.status_rx().await;
        debug!("Waiting for the VPN to disconnect...");


        while status_rx.changed().await.is_ok() {
            if *status_rx.borrow() == VpnStatus::Disconnected {
                info!("VPN disconnected");
                break;
            }
        }
    }

    pub async fn status(&self) -> VpnStatus {
        self.status_holder.lock().await.status
    }
}