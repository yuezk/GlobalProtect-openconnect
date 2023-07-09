import { atom } from "jotai";
import { connectPortalAtom } from "./connectPortal";
import {
  GatewayData,
  currentPortalDataAtom,
  updatePortalDataAtom,
} from "./portal";
import { statusAtom } from "./status";
import { disconnectVpnAtom } from "./vpn";

export const portalGatewaysAtom = atom<GatewayData[]>((get) => {
  const { gateways } = get(currentPortalDataAtom);
  return gateways;
});

export const selectedGatewayAtom = atom(
  (get) => get(currentPortalDataAtom).selectedGateway,
  async (get, set, update: string) => {
    const portalData = get(currentPortalDataAtom);
    await set(updatePortalDataAtom, { ...portalData, selectedGateway: update });
  }
);

export const gatewaySwitcherVisibleAtom = atom(false);
export const openGatewaySwitcherAtom = atom(null, (_get, set) => {
  set(gatewaySwitcherVisibleAtom, true);
});

const switchingAtom = atom(false);
export const switchGatewayAtom = atom(
  (get) => get(switchingAtom),
  async (get, set, gateway: GatewayData) => {
    const status = await get(statusAtom);

    // Update the selected gateway first
    await set(selectedGatewayAtom, gateway.name);

    if (status === "connected") {
      try {
        set(switchingAtom, true);
        await set(disconnectVpnAtom);
        await set(connectPortalAtom);
      } finally {
        set(switchingAtom, false);
      }
    }
  }
);
