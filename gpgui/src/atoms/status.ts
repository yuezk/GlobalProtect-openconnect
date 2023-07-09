import { atom } from "jotai";
import { atomWithDefault } from "jotai/utils";
import vpnService from "../services/vpnService";
import { selectedGatewayAtom, switchGatewayAtom } from "./gateway";
import { notifyErrorAtom, notifySuccessAtom } from "./notification";
import { unwrap } from "./unwrap";

export type Status =
  | "disconnected"
  | "prelogin"
  | "authenticating-saml"
  | "authenticating-password"
  | "portal-config"
  | "gateway-login"
  | "connecting"
  | "connected"
  | "disconnecting"
  | "error";

// Whether the gpservice has started
const _backgroundServiceStartedAtom = atomWithDefault<
  boolean | Promise<boolean>
>(() => vpnService.isOnline());

export const backgroundServiceStartedAtom = atom(
  (get) => get(_backgroundServiceStartedAtom),
  async (get, set, update: boolean) => {
    const prev = await get(_backgroundServiceStartedAtom);
    // Already started, do nothing
    if (update && update === prev) {
      return;
    }

    set(_backgroundServiceStartedAtom, update);
    // From stopped to started
    if (update) {
      set(notifySuccessAtom, "The background service is online");
    } else {
      set(notifyErrorAtom, "The background service is offline", 0);
    }
  }
);

backgroundServiceStartedAtom.onMount = (setAtom) => {
  vpnService.onServiceStatusChanged(setAtom);
};

// The current status of the vpn connection
export const statusAtom = atomWithDefault<Status | Promise<Status>>(() =>
  vpnService.status()
);

statusAtom.onMount = (setAtom) => vpnService.onVpnStatusChanged(setAtom);

const statusTextMap: Record<Status, string> = {
  disconnected: "Not Connected",
  prelogin: "Portal pre-logging in...",
  "authenticating-saml": "Authenticating...",
  "authenticating-password": "Authenticating...",
  "portal-config": "Retrieving portal config...",
  "gateway-login": "Logging in to gateway...",
  connecting: "Connecting...",
  connected: "Connected",
  disconnecting: "Disconnecting...",
  error: "Error",
};

export const statusTextAtom = atom<string>((get) => {
  const status = get(unwrap(statusAtom));
  const switchingGateway = get(switchGatewayAtom);

  if (!status) {
    return "Loading...";
  }

  if (status === "connected") {
    const selectedGateway = get(selectedGatewayAtom);
    return selectedGateway
      ? `Gateway: ${selectedGateway}`
      : statusTextMap[status];
  }

  if (switchingGateway) {
    const selectedGateway = get(selectedGatewayAtom);
    return `Switching to ${selectedGateway}`;
  }

  return statusTextMap[status];
});

export const isProcessingAtom = atom<boolean>((get) => {
  const status = get(unwrap(statusAtom));
  const switchingGateway = get(switchGatewayAtom);

  if (!status) {
    return false;
  }

  if (switchingGateway) {
    return true;
  }
  return status !== "disconnected" && status !== "connected";
});
