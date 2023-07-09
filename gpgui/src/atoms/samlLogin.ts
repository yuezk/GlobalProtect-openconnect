import { atom } from "jotai";
import authService, { AuthData } from "../services/authService";
import portalService, { SamlPrelogin } from "../services/portalService";
import { loginPortalAtom } from "./loginPortal";
import { clearCookiesAtom, portalAddressAtom } from "./portal";
import { statusAtom } from "./status";
import { unwrap } from "./unwrap";

export const launchSamlLoginAtom = atom(
  null,
  async (get, set, prelogin: SamlPrelogin) => {
    const { samlAuthMethod, samlRequest } = prelogin;
    let authData: AuthData;

    try {
      set(statusAtom, "authenticating-saml");
      const clearCookies = await get(clearCookiesAtom);
      authData = await authService.samlLogin(
        samlAuthMethod,
        samlRequest,
        clearCookies
      );

      // update clearCookies to false to reuse the SAML session
      await set(clearCookiesAtom, false);
    } catch (err) {
      throw new Error("SAML login failed");
    }

    if (!authData) {
      // User closed the SAML login window, cancel the login
      set(statusAtom, "disconnected");
      return;
    }

    const credential = {
      user: authData.username,
      "prelogin-cookie": authData.prelogin_cookie,
      "portal-userauthcookie": authData.portal_userauthcookie,
    };

    await set(loginPortalAtom, credential, prelogin);
  }
);

export const retrySamlLoginAtom = atom(null, async (get) => {
  const portal = get(portalAddressAtom);
  if (!portal) {
    throw new Error("Portal not found");
  }

  const prelogin = await portalService.prelogin(portal);
  if (prelogin.isSamlAuth) {
    await authService.emitAuthRequest({
      samlBinding: prelogin.samlAuthMethod,
      samlRequest: prelogin.samlRequest,
    });
  }
});
