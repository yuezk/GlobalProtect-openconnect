import { atom } from "jotai";
import gatewayService from "../services/gatewayService";
import { isProcessingAtom, statusAtom } from "./status";
import { connectVpnAtom } from "./vpn";

type GatewayCredential = {
  user: string;
  passwd?: string;
  userAuthCookie: string;
  prelogonUserAuthCookie: string;
};

/**
 * Login to a gateway to get the token, and then connect to VPN with the token
 */
export const loginGatewayAtom = atom(
  null,
  async (get, set, gateway: string, credential: GatewayCredential) => {
    set(statusAtom, "gateway-login");
    let token: string;
    try {
      token = await gatewayService.login(gateway, credential);
    } catch (err) {
      throw new Error("Failed to login to gateway");
    }

    const isProcessing = get(isProcessingAtom);
    if (!isProcessing) {
      console.info("Request cancelled");
      return;
    }

    await set(connectVpnAtom, gateway, token);
  }
);
