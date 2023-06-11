import { Button, TextField } from "@mui/material";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { ChangeEvent } from "react";
import { disconnectVpnAtom } from "../../atoms/gateway";
import {
  cancelConnectPortalAtom,
  connectPortalAtom,
  portalAddressAtom,
  switchingGatewayAtom,
} from "../../atoms/portal";
import { isOnlineAtom, statusAtom } from "../../atoms/status";

export default function PortalForm() {
  const isOnline = useAtomValue(isOnlineAtom);
  const [portalAddress, setPortalAddress] = useAtom(portalAddressAtom);
  const status = useAtomValue(statusAtom);
  const [processing, connectPortal] = useAtom(connectPortalAtom);
  const cancelConnectPortal = useSetAtom(cancelConnectPortalAtom);
  const disconnectVpn = useSetAtom(disconnectVpnAtom);
  const switchingGateway = useAtomValue(switchingGatewayAtom);

  function handlePortalAddressChange(e: ChangeEvent<HTMLInputElement>) {
    let host = e.target.value.trim();
    if (/^https?:\/\//.test(host)) {
      try {
        host = new URL(host).hostname;
      } catch (e) {}
    }
    setPortalAddress(host);
  }

  function handleSubmit(e: ChangeEvent<HTMLFormElement>) {
    e.preventDefault();
    connectPortal();
  }

  return (
    <form onSubmit={handleSubmit}>
      <TextField
        autoFocus
        label="Portal address"
        placeholder="Hostname or IP address"
        fullWidth
        size="small"
        value={portalAddress}
        onChange={handlePortalAddressChange}
        InputProps={{ readOnly: status !== "disconnected" || switchingGateway }}
        sx={{ mb: 1 }}
      />
      {status === "disconnected" && !switchingGateway && (
        <Button
          fullWidth
          type="submit"
          variant="contained"
          disabled={!isOnline}
          sx={{ textTransform: "none" }}
        >
          Connect
        </Button>
      )}
      {(processing || switchingGateway) && (
        <Button
          fullWidth
          variant="outlined"
          disabled={
            status === "authenticating-saml" ||
            status === "connecting" ||
            status === "disconnecting" ||
            switchingGateway
          }
          onClick={cancelConnectPortal}
          sx={{ textTransform: "none" }}
        >
          Cancel
        </Button>
      )}
      {status === "connected" && (
        <Button
          fullWidth
          variant="contained"
          onClick={disconnectVpn}
          sx={{ textTransform: "none" }}
        >
          Disconnect
        </Button>
      )}
    </form>
  );
}
