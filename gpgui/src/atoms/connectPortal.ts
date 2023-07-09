import { atom } from "jotai";
import authService from "../services/authService";
import portalService, { Prelogin } from "../services/portalService";
import { loginPortalAtom } from "./loginPortal";
import { notifyErrorAtom } from "./notification";
import { launchPasswordLoginAtom } from "./passwordLogin";
import { currentPortalDataAtom, portalAddressAtom } from "./portal";
import { launchSamlLoginAtom, retrySamlLoginAtom } from "./samlLogin";
import { isProcessingAtom, statusAtom } from "./status";

/**
 * Connect to the portal, workflow:
 * 1. Portal prelogin to get the prelogin data
 * 2. Try to login with the cached credential
 * 3. If login failed, launch the SAML login window or the password login window based on the prelogin data
 */
export const connectPortalAtom = atom(
  null,
  async (get, set, action?: "retry-auth") => {
    // Retry the SAML authentication
    if (action === "retry-auth") {
      set(retrySamlLoginAtom);
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
        // If the portal is cached, use the cached credential
        await set(loginWithCachedCredentialAtom, prelogin);
      } catch {
        // Otherwise, login with SAML or the password
        if (prelogin.isSamlAuth) {
          await set(launchSamlLoginAtom, prelogin);
        } else {
          set(launchPasswordLoginAtom, prelogin);
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

export const cancelConnectPortalAtom = atom(null, (_get, set) => {
  set(statusAtom, "disconnected");
});

/**
 * Read the cached credential from the current portal data and login with it
 */
const loginWithCachedCredentialAtom = atom(
  null,
  async (get, set, prelogin: Prelogin) => {
    const { cachedCredential } = get(currentPortalDataAtom);
    if (!cachedCredential) {
      throw new Error("No cached credential");
    }

    await set(loginPortalAtom, cachedCredential, prelogin);
  }
);
