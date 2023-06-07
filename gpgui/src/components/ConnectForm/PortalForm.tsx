import { Button, TextField } from "@mui/material";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { ChangeEvent } from "react";
import { disconnectVpnAtom } from "../../atoms/gateway";
import {
  cancelConnectPortalAtom,
  connectPortalAtom,
  portalAtom,
} from "../../atoms/portal";
import { statusAtom } from "../../atoms/status";

export default function PortalForm() {
  const [portal, setPortal] = useAtom(portalAtom);
  const status = useAtomValue(statusAtom);
  const [processing, connectPortal] = useAtom(connectPortalAtom);
  const cancelConnectPortal = useSetAtom(cancelConnectPortalAtom);
  const disconnectVpn = useSetAtom(disconnectVpnAtom);

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
        value={portal}
        onChange={(e) => setPortal(e.target.value.trim())}
        InputProps={{ readOnly: status !== "disconnected" }}
        sx={{ mb: 1 }}
      />
      {status === "disconnected" && (
        <Button
          fullWidth
          type="submit"
          variant="contained"
          sx={{ textTransform: "none" }}
        >
          Connect
        </Button>
      )}
      {processing && (
        <Button
          fullWidth
          variant="outlined"
          disabled={status === "authenticating-saml"}
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
