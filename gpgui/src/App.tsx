import { Box, TextField } from "@mui/material";
import Button from "@mui/material/Button";
import { ChangeEvent, FormEvent, useEffect, useState } from "react";

import "./App.css";
import ConnectionStatus, { Status } from "./components/ConnectionStatus";
import Notification, { NotificationConfig } from "./components/Notification";
import PasswordAuth, {
  Credentials,
  PasswordAuthData,
} from "./components/PasswordAuth";
import gatewayService from "./services/gatewayService";
import portalService from "./services/portalService";
import vpnService from "./services/vpnService";

export default function App() {
  const [portalAddress, setPortalAddress] = useState("220.191.185.154");
  const [status, setStatus] = useState<Status>("disconnected");
  const [processing, setProcessing] = useState(false);
  const [passwordAuthOpen, setPasswordAuthOpen] = useState(false);
  const [passwordAuthenticating, setPasswordAuthenticating] = useState(false);
  const [passwordAuth, setPasswordAuth] = useState<PasswordAuthData>();
  const [notification, setNotification] = useState<NotificationConfig>({
    open: false,
    message: "",
  });

  useEffect(() => {
    return vpnService.onStatusChanged((latestStatus) => {
      console.log("status changed", latestStatus);
      setStatus(latestStatus);
      if (latestStatus === "connected") {
        clearOverlays();
      }
    });
  }, []);

  function closeNotification() {
    setNotification((notification) => ({
      ...notification,
      open: false,
    }));
  }

  function clearOverlays() {
    closeNotification();
    setPasswordAuthenticating(false);
    setPasswordAuthOpen(false);
  }

  function handlePortalChange(e: ChangeEvent<HTMLInputElement>) {
    const { value } = e.target;
    setPortalAddress(value.trim());
  }

  async function handleConnect(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();

    setProcessing(true);

    try {
      const response = await portalService.prelogin(portalAddress);

      if (portalService.isSamlAuth(response)) {
        // TODO SAML login
      } else if (portalService.isPasswordAuth(response)) {
        setPasswordAuthOpen(true);
        setPasswordAuth({
          authMessage: response.authMessage,
          labelPassword: response.labelPassword,
          labelUsername: response.labelUsername,
        });
      } else {
        throw new Error("Unsupported portal login method");
      }
    } catch (e) {
      setProcessing(false);
    }
  }

  function handleCancel() {
    // TODO cancel the request first
    setProcessing(false);
  }

  async function handleDisconnect() {
    setProcessing(true);

    try {
      await vpnService.disconnect();
    } catch (err: any) {
      setNotification({
        open: true,
        type: "error",
        title: "Failed to disconnect",
        message: err.message,
      });
    } finally {
      setProcessing(false);
    }
  }

  async function handlePasswordAuth({ username, password }: Credentials) {
    try {
      setPasswordAuthenticating(true);
      const portalConfigResponse = await portalService.fetchConfig({
        portal: portalAddress,
        username,
        password,
      });

      const { gateways, preferredGateway, userAuthCookie } =
        portalConfigResponse;

      if (gateways.length === 0) {
        // TODO handle no gateways, treat the portal as a gateway
        throw new Error("No gateways found");
      }

      const token = await gatewayService.login({
        gateway: preferredGateway,
        username,
        password,
        userAuthCookie,
      });

      await vpnService.connect(preferredGateway.address!, token);
      setProcessing(false);
    } catch (err: any) {
      console.error(err);
      setNotification({
        open: true,
        type: "error",
        title: "Login failed",
        message: err.message,
      });
    } finally {
      setPasswordAuthenticating(false);
    }
  }

  function cancelPasswordAuth() {
    setPasswordAuthenticating(false);
    setPasswordAuthOpen(false);
    setProcessing(false);
  }
  return (
    <Box padding={2} paddingTop={3}>
      <ConnectionStatus
        sx={{ mb: 2 }}
        status={processing ? "processing" : status}
      />

      <form onSubmit={handleConnect}>
        <TextField
          autoFocus
          label="Portal address"
          placeholder="Hostname or IP address"
          fullWidth
          size="small"
          value={portalAddress}
          onChange={handlePortalChange}
          InputProps={{ readOnly: status !== "disconnected" }}
        />
        <Box sx={{ mt: 1.5 }}>
          {status === "disconnected" && (
            <Button
              type="submit"
              variant="contained"
              fullWidth
              sx={{ textTransform: "none" }}
            >
              Connect
            </Button>
          )}
          {status === "connecting" && (
            <Button
              variant="outlined"
              fullWidth
              onClick={handleCancel}
              sx={{ textTransform: "none" }}
            >
              Cancel
            </Button>
          )}
          {status === "connected" && (
            <Button
              variant="contained"
              fullWidth
              onClick={handleDisconnect}
              sx={{ textTransform: "none" }}
            >
              Disconnect
            </Button>
          )}
        </Box>
      </form>

      <PasswordAuth
        open={passwordAuthOpen}
        authData={passwordAuth}
        authenticating={passwordAuthenticating}
        onCancel={cancelPasswordAuth}
        onLogin={handlePasswordAuth}
      />
      <Notification {...notification} onClose={closeNotification} />
    </Box>
  );
}
