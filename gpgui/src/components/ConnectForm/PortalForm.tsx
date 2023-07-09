import { Button, TextField } from "@mui/material";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { ChangeEvent } from "react";
import {
  cancelConnectPortalAtom,
  connectPortalAtom,
} from "../../atoms/connectPortal";
import { switchGatewayAtom } from "../../atoms/gateway";
import { portalAddressAtom } from "../../atoms/portal";
import {
  backgroundServiceStartedAtom,
  isProcessingAtom,
  statusAtom,
} from "../../atoms/status";
import { disconnectVpnAtom } from "../../atoms/vpn";

function normalizePortalAddress(input: string) {
  const address = input.trim();
  if (/^https?:\/\//.test(address)) {
    try {
      return new URL(address).hostname;
    } catch (e) {}
  }
  return address;
}

export default function PortalForm() {
  const backgroundServiceStarted = useAtomValue(backgroundServiceStartedAtom);
  const [portalAddress, setPortalAddress] = useAtom(portalAddressAtom);
  // Use useAtom instead of useSetAtom, otherwise the onMount of the atom is not triggered
  const [, connectPortal] = useAtom(connectPortalAtom);
  const cancelConnectPortal = useSetAtom(cancelConnectPortalAtom);
  const isProcessing = useAtomValue(isProcessingAtom);
  const status = useAtomValue(statusAtom);
  const disconnectVpn = useSetAtom(disconnectVpnAtom);
  const switchingGateway = useAtomValue(switchGatewayAtom);

  function handlePortalAddressChange(e: ChangeEvent<HTMLInputElement>) {
    setPortalAddress(normalizePortalAddress(e.target.value));
  }

  function handleSubmit(e: ChangeEvent<HTMLFormElement>) {
    e.preventDefault();
    connectPortal();
  }

  return (
    <form onSubmit={handleSubmit} data-tauri-drag-region>
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
          disabled={!backgroundServiceStarted}
          sx={{ textTransform: "none" }}
        >
          Connect
        </Button>
      )}

      {isProcessing && (
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
