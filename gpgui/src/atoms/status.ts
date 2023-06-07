import { atom } from "jotai";
import vpnService from "../services/vpnService";

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

export const statusAtom = atom<Status>("disconnected");
statusAtom.onMount = (setAtom) => {
  return vpnService.onStatusChanged((status) => {
    status === "connected" && setAtom("connected");
  });
};

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
  return statusTextMap[status];
});

export const isProcessingAtom = atom((get) => {
  const status = get(statusAtom);
  return status !== "disconnected" && status !== "connected";
});
