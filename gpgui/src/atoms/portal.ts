import { atom } from "jotai";
import { focusAtom } from "jotai-optics";
import authService, { AuthData } from "../services/authService";
import portalService, {
  PasswordPrelogin,
  Prelogin,
  SamlPrelogin,
} from "../services/portalService";
import { gatewayLoginAtom } from "./gateway";
import { notifyErrorAtom } from "./notification";
import { isProcessingAtom, statusAtom } from "./status";

type GatewayData = {
  name: string;
  address: string;
};

type Credential = {
  user: string;
  passwd: string;
  userAuthCookie: string;
  prelogonUserAuthCookie: string;
};

type AppData = {
  portal: string;
  gateways: GatewayData[];
  selectedGateway: string;
  credentials: Record<string, Credential>;
};

const appAtom = atom<AppData>({
  portal: "",
  gateways: [],
  selectedGateway: "",
  credentials: {},
});

export const portalAtom = focusAtom(appAtom, (optic) => optic.prop("portal"));
export const connectPortalAtom = atom(
  (get) => get(isProcessingAtom),
  async (get, set, action?: "retry-auth") => {
    // Retry the SAML authentication
    if (action === "retry-auth") {
      set(retrySamlAuthAtom);
      return;
    }

    const portal = get(portalAtom);
    if (!portal) {
      set(notifyErrorAtom, "Portal is empty");
      return;
    }

    try {
      set(statusAtom, "prelogin");
      const prelogin = await portalService.prelogin(portal);
      if (!get(isProcessingAtom)) {
        console.info("Request cancelled");
        return;
      }

      if (prelogin.isSamlAuth) {
        await set(launchSamlAuthAtom, prelogin);
      } else {
        await set(launchPasswordAuthAtom, prelogin);
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

export const usernameAtom = atom("");
export const passwordAtom = atom("");
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
    const portal = get(portalAtom);
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
  async (_get, set, prelogin: SamlPrelogin) => {
    const { samlAuthMethod, samlRequest } = prelogin;
    let authData: AuthData;

    try {
      set(statusAtom, "authenticating-saml");
      authData = await authService.samlLogin(samlAuthMethod, samlRequest);
    } catch (err) {
      throw new Error("SAML login failed");
    }

    if (!authData) {
      // User closed the SAML login window, cancel the login
      set(cancelConnectPortalAtom);
      return;
    }

    const credential = {
      user: authData.username,
      "prelogin-cookie": authData.prelogin_cookie,
      "portal-userauthcookie": authData.portal_userauthcookie,
    };
    await set(portalLoginAtom, credential, prelogin);
  }
);

const retrySamlAuthAtom = atom(null, async (get) => {
  const portal = get(portalAtom);
  const prelogin = await portalService.prelogin(portal);
  if (prelogin.isSamlAuth) {
    await authService.emitAuthRequest({
      samlBinding: prelogin.samlAuthMethod,
      samlRequest: prelogin.samlRequest,
    });
  }
});

type PortalCredential =
  | {
      user: string;
      passwd: string;
    }
  | {
      user: string;
      "prelogin-cookie": string | null;
      "portal-userauthcookie": string | null;
    };

const portalConfigLoadingAtom = atom(false);
const portalLoginAtom = atom(
  (get) => get(portalConfigLoadingAtom),
  async (get, set, credential: PortalCredential, prelogin: Prelogin) => {
    set(statusAtom, "portal-config");
    set(portalConfigLoadingAtom, true);

    const portal = get(portalAtom);
    let portalConfig;
    try {
      portalConfig = await portalService.fetchConfig(portal, credential);
      // Ensure the password auth window is closed
      set(passwordAuthVisibleAtom, false);
    } finally {
      set(portalConfigLoadingAtom, false);
    }

    if (!get(isProcessingAtom)) {
      console.info("Request cancelled");
      return;
    }

    const { gateways, userAuthCookie, prelogonUserAuthCookie } = portalConfig;
    console.info("portalConfig", portalConfig);
    if (!gateways.length) {
      throw new Error("No gateway found");
    }

    const { region } = prelogin;
    const { address } = portalService.preferredGateway(gateways, region);
    await set(gatewayLoginAtom, address, {
      user: credential.user,
      userAuthCookie,
      prelogonUserAuthCookie,
    });
  }
);
