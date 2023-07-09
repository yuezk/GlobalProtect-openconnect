import { atom } from "jotai";
import { atomWithDefault } from "jotai/utils";
import { PortalCredential } from "../services/portalService";
import { atomWithTauriStorage } from "../services/storeService";
import { unwrap } from "./unwrap";

export type GatewayData = {
  name: string;
  address: string;
};

type CachedPortalCredential = Omit<PortalCredential, "prelogin-cookie">;

type PortalData = {
  address: string;
  gateways: GatewayData[];
  cachedCredential?: CachedPortalCredential;
  selectedGateway?: string;
};

type AppData = {
  portal: string;
  portals: PortalData[];
  clearCookies: boolean;
};

const DEFAULT_APP_DATA: AppData = {
  portal: "",
  portals: [],
  // Whether to clear the cookies of the SAML login webview, default is true
  clearCookies: true,
};

export const appDataAtom = atomWithTauriStorage("APP_DATA", DEFAULT_APP_DATA);
const unwrappedAppDataAtom = atom(
  (get) => get(unwrap(appDataAtom)) || DEFAULT_APP_DATA
);

// Read the portal address from the store as the default value
export const portalAddressAtom = atomWithDefault<string>(
  (get) => get(unwrappedAppDataAtom).portal
);

// The cached portal data for the current portal address
export const currentPortalDataAtom = atom<PortalData>((get) => {
  const portalAddress = get(portalAddressAtom);
  const appData = get(unwrappedAppDataAtom);
  const { portals } = appData;
  const portalData = portals.find(({ address }) => address === portalAddress);

  return portalData || { address: portalAddress, gateways: [] };
});

export const updatePortalDataAtom = atom(
  null,
  async (get, set, update: PortalData) => {
    const appData = await get(appDataAtom);
    const { portals } = appData;
    const portalIndex = portals.findIndex(
      ({ address }) => address === update.address
    );

    if (portalIndex === -1) {
      portals.push(update);
    } else {
      portals[portalIndex] = update;
    }

    await set(appDataAtom, (appData) => ({
      ...appData,
      portal: update.address,
      portals,
    }));
  }
);

export const clearCookiesAtom = atom(
  async (get) => {
    const { clearCookies } = await get(appDataAtom);
    return clearCookies;
  },
  async (_get, set, update: boolean) => {
    await set(appDataAtom, (appData) => ({
      ...appData,
      clearCookies: update,
    }));
  }
);
