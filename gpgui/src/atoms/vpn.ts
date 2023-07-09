import { atom } from "jotai";
import vpnService from "../services/vpnService";
import { notifyErrorAtom } from "./notification";
import { statusAtom } from "./status";

export const connectVpnAtom = atom(
  null,
  async (_get, set, vpnAddress: string, token: string) => {
    try {
      set(statusAtom, "connecting");
      await vpnService.connect(vpnAddress, token);
      set(statusAtom, "connected");
    } catch (err) {
      throw new Error("Failed to connect to VPN");
    }
  }
);

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
export const disconnectVpnAtom = atom(null, async (get, set) => {
  try {
    set(statusAtom, "disconnecting");
    await vpnService.disconnect();
    // Sleep a short time, so that the client can receive the service's disconnected event.
    await sleep(100);
  } catch (err) {
    set(statusAtom, "disconnected");
    set(notifyErrorAtom, "Failed to disconnect from VPN");
  }
});
