import { Autocomplete, Button, TextField } from "@mui/material";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { ChangeEvent, useState } from "react";
import {
  cancelConnectPortalAtom,
  connectPortalAtom,
} from "../../atoms/connectPortal";
import { switchGatewayAtom } from "../../atoms/gateway";
import { allPortalsAtom, portalAddressAtom } from "../../atoms/portal";
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
  const allPortals = useAtomValue(allPortalsAtom);
  const [portalAddress, setPortalAddress] = useAtom(portalAddressAtom);
  // Use useAtom instead of useSetAtom, otherwise the onMount of the atom is not triggered
  const [, connectPortal] = useAtom(connectPortalAtom);
  const cancelConnectPortal = useSetAtom(cancelConnectPortalAtom);
  const isProcessing = useAtomValue(isProcessingAtom);
  const status = useAtomValue(statusAtom);
  const disconnectVpn = useSetAtom(disconnectVpnAtom);
  const switchingGateway = useAtomValue(switchGatewayAtom);

  const readOnly = status !== "disconnected" || switchingGateway;

  function handlePortalAddressChange(e: unknown, value: string) {
    setPortalAddress(normalizePortalAddress(value));
  }

  function handleSubmit(e: ChangeEvent<HTMLFormElement>) {
    e.preventDefault();
    connectPortal();
  }

  return (
    <form onSubmit={handleSubmit} data-tauri-drag-region>
      <Autocomplete
        freeSolo
        options={allPortals}
        inputValue={portalAddress}
        onInputChange={handlePortalAddressChange}
        readOnly={readOnly}
        forcePopupIcon={!readOnly}
        disableClearable
        size="small"
        sx={{
          mb: 1,
        }}
        componentsProps={{
          paper: {
            sx: {
              "& .MuiAutocomplete-listbox .MuiAutocomplete-option": {
                minHeight: "auto",
              },
            },
          },
        }}
        renderInput={(params) => (
          <TextField
            {...params}
            autoFocus
            label="Portal address"
            placeholder="Hostname or IP address"
          />
        )}
      />

      {status === "disconnected" && !switchingGateway && (
        <Button
          fullWidth
          type="submit"
          variant="contained"
          disabled={!backgroundServiceStarted}
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
        >
          Cancel
        </Button>
      )}

      {status === "connected" && (
        <Button fullWidth variant="contained" onClick={disconnectVpn}>
          Disconnect
        </Button>
      )}
    </form>
  );
}
