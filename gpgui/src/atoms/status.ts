import { atom } from "jotai";
import { atomWithDefault } from "jotai/utils";
import vpnService from "../services/vpnService";
import { notifyErrorAtom, notifySuccessAtom } from "./notification";
import { selectedGatewayAtom, switchingGatewayAtom } from "./portal";

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

const internalIsOnlineAtom = atomWithDefault(() => vpnService.isOnline());
export const isOnlineAtom = atom(
  (get) => get(internalIsOnlineAtom),
  async (get, set, update: boolean) => {
    const isOnline = await get(internalIsOnlineAtom);
    // Already online, do nothing
    if (update && update === isOnline) {
      return;
    }

    set(internalIsOnlineAtom, update);
    if (update) {
      set(notifySuccessAtom, "The background service is online");
    } else {
      set(notifyErrorAtom, "The background service is offline", 0);
    }
  }
);
isOnlineAtom.onMount = (setAtom) => vpnService.onServiceStatusChanged(setAtom);

const internalStatusReadyAtom = atom(false);
export const statusReadyAtom = atom(
  (get) => get(internalStatusReadyAtom),
  (get, set, status: Status) => {
    set(internalStatusReadyAtom, true);
    set(statusAtom, status);
  }
);

statusReadyAtom.onMount = (setAtom) => {
  vpnService.status().then(setAtom);
};

export const statusAtom = atom<Status>("disconnected");
statusAtom.onMount = (setAtom) => vpnService.onVpnStatusChanged(setAtom);

const statusTextMap: Record<Status, String> = {
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

export const statusTextAtom = atom((get) => {
  const status = get(statusAtom);
  const switchingGateway = get(switchingGatewayAtom);

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

export const isProcessingAtom = atom((get) => {
  const status = get(statusAtom);
  const switchingGateway = get(switchingGatewayAtom);

  return (
    (status !== "disconnected" && status !== "connected") || switchingGateway
  );
});
