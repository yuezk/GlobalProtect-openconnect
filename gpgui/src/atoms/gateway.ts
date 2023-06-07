import { atom } from "jotai";
import gatewayService from "../services/gatewayService";
import vpnService from "../services/vpnService";
import { notifyErrorAtom } from "./notification";
import { isProcessingAtom, statusAtom } from "./status";

type GatewayCredential = {
  user: string;
  passwd?: string;
  userAuthCookie: string;
  prelogonUserAuthCookie: string;
};

export const gatewayLoginAtom = atom(
  null,
  async (get, set, gateway: string, credential: GatewayCredential) => {
    set(statusAtom, "gateway-login");
    let token: string;
    try {
      token = await gatewayService.login(gateway, credential);
    } catch (err) {
      throw new Error("Failed to login to gateway");
    }

    if (!get(isProcessingAtom)) {
      console.info("Request cancelled");
      return;
    }

    await set(connectVpnAtom, gateway, token);
  }
);

const connectVpnAtom = atom(
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

export const disconnectVpnAtom = atom(null, async (get, set) => {
  try {
    set(statusAtom, "disconnecting");
    await vpnService.disconnect();
    set(statusAtom, "disconnected");
  } catch (err) {
    set(statusAtom, "disconnected");
    set(notifyErrorAtom, "Failed to disconnect from VPN");
  }
});
