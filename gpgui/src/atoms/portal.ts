import { atom } from "jotai";
import { withImmer } from "jotai-immer";
import { atomWithDefault, atomWithStorage } from "jotai/utils";
import authService, { AuthData } from "../services/authService";
import portalService, {
  PasswordPrelogin,
  PortalCredential,
  Prelogin,
  SamlPrelogin,
} from "../services/portalService";
import { disconnectVpnAtom, gatewayLoginAtom } from "./gateway";
import { notifyErrorAtom } from "./notification";
import { isProcessingAtom, statusAtom } from "./status";

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

type AppDataUpdate =
  | {
      type: "PORTAL";
      payload: PortalData;
    }
  | {
      type: "SELECTED_GATEWAY";
      payload: string;
    };

const defaultAppData: AppData = {
  portal: "",
  portals: [],
  // Whether to clear the cookies of the SAML login webview, default is true
  clearCookies: true,
};

export const appDataStorageAtom = atomWithStorage<AppData>(
  "APP_DATA",
  defaultAppData
);
const appDataImmerAtom = withImmer(appDataStorageAtom);

const updateAppDataAtom = atom(null, (_get, set, update: AppDataUpdate) => {
  const { type, payload } = update;
  switch (type) {
    case "PORTAL":
      const { address } = payload;
      set(appDataImmerAtom, (draft) => {
        draft.portal = address;
        const portalIndex = draft.portals.findIndex(
          ({ address: portalAddress }) => portalAddress === address
        );
        if (portalIndex === -1) {
          draft.portals.push(payload);
        } else {
          draft.portals[portalIndex] = payload;
        }
      });
      break;
    case "SELECTED_GATEWAY":
      set(appDataImmerAtom, (draft) => {
        const { portal, portals } = draft;
        const portalData = portals.find(({ address }) => address === portal);
        if (portalData) {
          portalData.selectedGateway = payload;
        }
      });
      break;
  }
});

export const portalAddressAtom = atomWithDefault(
  (get) => get(appDataImmerAtom).portal
);

export const currentPortalDataAtom = atom<PortalData>((get) => {
  const portalAddress = get(portalAddressAtom);
  const { portals } = get(appDataImmerAtom);
  const portalData = portals.find(({ address }) => address === portalAddress);

  return portalData || { address: portalAddress, gateways: [] };
});

const clearCookiesAtom = atom(
  (get) => get(appDataImmerAtom).clearCookies,
  (_get, set, update: boolean) => {
    set(appDataImmerAtom, (draft) => {
      draft.clearCookies = update;
    });
  }
);

export const portalGatewaysAtom = atom<GatewayData[]>((get) => {
  const { gateways } = get(currentPortalDataAtom);
  return gateways;
});

export const selectedGatewayAtom = atom(
  (get) => get(currentPortalDataAtom).selectedGateway
);

export const connectPortalAtom = atom(
  (get) => get(isProcessingAtom),
  async (get, set, action?: "retry-auth") => {
    // Retry the SAML authentication
    if (action === "retry-auth") {
      set(retrySamlAuthAtom);
      return;
    }

    const portal = get(portalAddressAtom);
    if (!portal) {
      set(notifyErrorAtom, "Portal is empty");
      return;
    }

    try {
      set(statusAtom, "prelogin");
      const prelogin = await portalService.prelogin(portal);
      const isProcessing = get(isProcessingAtom);
      if (!isProcessing) {
        console.info("Request cancelled");
        return;
      }

      try {
        await set(loginWithCachedCredentialAtom, prelogin);
      } catch {
        if (prelogin.isSamlAuth) {
          await set(launchSamlAuthAtom, prelogin);
        } else {
          await set(launchPasswordAuthAtom, prelogin);
        }
      }
    } catch (err) {
      set(cancelConnectPortalAtom);
      set(notifyErrorAtom, err);
    }
  }
);

connectPortalAtom.onMount = (dispatch) => {
  return authService.onAuthError(() => {
    dispatch("retry-auth");
  });
};

const loginWithCachedCredentialAtom = atom(
  null,
  async (get, set, prelogin: Prelogin) => {
    const { cachedCredential } = get(currentPortalDataAtom);
    if (!cachedCredential) {
      throw new Error("No cached credential");
    }
    await set(portalLoginAtom, cachedCredential, prelogin);
  }
);

export const passwordPreloginAtom = atom<PasswordPrelogin>({
  isSamlAuth: false,
  region: "",
  authMessage: "",
  labelUsername: "",
  labelPassword: "",
});

export const cancelConnectPortalAtom = atom(null, (_get, set) => {
  set(statusAtom, "disconnected");
});

export const usernameAtom = atomWithDefault(
  (get) => get(currentPortalDataAtom).cachedCredential?.user ?? ""
);

export const passwordAtom = atomWithDefault(
  (get) => get(currentPortalDataAtom).cachedCredential?.passwd ?? ""
);

const passwordAuthVisibleAtom = atom(false);

const launchPasswordAuthAtom = atom(
  null,
  async (_get, set, prelogin: PasswordPrelogin) => {
    set(passwordAuthVisibleAtom, true);
    set(passwordPreloginAtom, prelogin);
    set(statusAtom, "authenticating-password");
  }
);

export const cancelPasswordAuthAtom = atom(
  (get) => get(passwordAuthVisibleAtom),
  (_get, set) => {
    set(passwordAuthVisibleAtom, false);
    set(cancelConnectPortalAtom);
  }
);

export const passwordLoginAtom = atom(
  (get) => get(portalConfigLoadingAtom),
  async (get, set, username: string, password: string) => {
    const portal = get(portalAddressAtom);
    if (!portal) {
      set(notifyErrorAtom, "Portal is empty");
      return;
    }

    if (!username) {
      set(notifyErrorAtom, "Username is empty");
      return;
    }

    try {
      const credential = { user: username, passwd: password };
      const prelogin = get(passwordPreloginAtom);
      await set(portalLoginAtom, credential, prelogin);
    } catch (err) {
      set(cancelConnectPortalAtom);
      set(notifyErrorAtom, err);
    }
  }
);

const launchSamlAuthAtom = atom(
  null,
  async (get, set, prelogin: SamlPrelogin) => {
    const { samlAuthMethod, samlRequest } = prelogin;
    let authData: AuthData;

    try {
      set(statusAtom, "authenticating-saml");
      const clearCookies = get(clearCookiesAtom);
      authData = await authService.samlLogin(
        samlAuthMethod,
        samlRequest,
        clearCookies
      );
    } catch (err) {
      throw new Error("SAML login failed");
    }

    if (!authData) {
      // User closed the SAML login window, cancel the login
      set(cancelConnectPortalAtom);
      return;
    }

    // SAML login success, update clearCookies to false to reuse the SAML session
    set(clearCookiesAtom, false);

    const credential = {
      user: authData.username,
      "prelogin-cookie": authData.prelogin_cookie,
      "portal-userauthcookie": authData.portal_userauthcookie,
    };

    await set(portalLoginAtom, credential, prelogin);
  }
);

const retrySamlAuthAtom = atom(null, async (get) => {
  const portal = get(portalAddressAtom);
  const prelogin = await portalService.prelogin(portal);
  if (prelogin.isSamlAuth) {
    await authService.emitAuthRequest({
      samlBinding: prelogin.samlAuthMethod,
      samlRequest: prelogin.samlRequest,
    });
  }
});

const portalConfigLoadingAtom = atom(false);
const portalLoginAtom = atom(
  (get) => get(portalConfigLoadingAtom),
  async (get, set, credential: PortalCredential, prelogin: Prelogin) => {
    set(statusAtom, "portal-config");
    set(portalConfigLoadingAtom, true);

    const portalAddress = get(portalAddressAtom);
    let portalConfig;
    try {
      portalConfig = await portalService.fetchConfig(portalAddress, credential);
      // Ensure the password auth window is closed
      set(passwordAuthVisibleAtom, false);
    } finally {
      set(portalConfigLoadingAtom, false);
    }

    const isProcessing = get(isProcessingAtom);
    if (!isProcessing) {
      console.info("Request cancelled");
      return;
    }

    const { gateways, userAuthCookie, prelogonUserAuthCookie } = portalConfig;
    if (!gateways.length) {
      throw new Error("No gateway found");
    }

    if (userAuthCookie === "empty" || prelogonUserAuthCookie === "empty") {
      throw new Error("Failed to login, please try again");
    }

    // Previous selected gateway
    const previousGateway = get(selectedGatewayAtom);
    // Update the app data to persist the portal data
    set(updateAppDataAtom, {
      type: "PORTAL",
      payload: {
        address: portalAddress,
        gateways: gateways.map(({ name, address }) => ({
          name,
          address,
        })),
        cachedCredential: {
          user: credential.user,
          passwd: credential.passwd,
          "portal-userauthcookie": userAuthCookie,
          "portal-prelogonuserauthcookie": prelogonUserAuthCookie,
        },
        selectedGateway: previousGateway,
      },
    });

    const { region } = prelogin;
    const { name, address } = portalService.preferredGateway(gateways, {
      region,
      previousGateway,
    });
    await set(gatewayLoginAtom, address, {
      user: credential.user,
      userAuthCookie,
      prelogonUserAuthCookie,
    });

    // Update the app data to persist the gateway data
    set(updateAppDataAtom, {
      type: "SELECTED_GATEWAY",
      payload: name,
    });
  }
);

export const switchingGatewayAtom = atom(false);
export const switchToGatewayAtom = atom(
  (get) => get(switchingGatewayAtom),
  async (get, set, gateway: GatewayData) => {
    set(updateAppDataAtom, {
      type: "SELECTED_GATEWAY",
      payload: gateway.name,
    });

    if (get(statusAtom) === "connected") {
      try {
        set(switchingGatewayAtom, true);
        await set(disconnectVpnAtom);
        await set(connectPortalAtom);
      } finally {
        set(switchingGatewayAtom, false);
      }
    }
  }
);
