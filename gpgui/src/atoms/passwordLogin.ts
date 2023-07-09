import { atom } from "jotai";
import { atomWithDefault } from "jotai/utils";
import { PasswordPrelogin } from "../services/portalService";
import { loginPortalAtom } from "./loginPortal";
import { notifyErrorAtom } from "./notification";
import { currentPortalDataAtom, portalAddressAtom } from "./portal";
import { statusAtom } from "./status";

const loginFormVisibleAtom = atom(false);

export const passwordPreloginAtom = atom<PasswordPrelogin>({
  isSamlAuth: false,
  region: "",
  authMessage: "",
  labelUsername: "",
  labelPassword: "",
});

export const launchPasswordLoginAtom = atom(
  null,
  (_get, set, prelogin: PasswordPrelogin) => {
    set(loginFormVisibleAtom, true);
    set(passwordPreloginAtom, prelogin);
    set(statusAtom, "authenticating-password");
  }
);

// Use the cached credential to login
export const usernameAtom = atomWithDefault((get) => {
  return get(currentPortalDataAtom).cachedCredential?.user ?? "";
});

export const passwordAtom = atomWithDefault((get) => {
  return get(currentPortalDataAtom).cachedCredential?.passwd ?? "";
});

export const cancelPasswordAuthAtom = atom(
  (get) => get(loginFormVisibleAtom),
  (_get, set) => {
    set(loginFormVisibleAtom, false);
    set(statusAtom, "disconnected");
  }
);

export const passwordLoginAtom = atom(
  (get) => get(loginPortalAtom),
  async (get, set) => {
    const portal = get(portalAddressAtom);
    const username = get(usernameAtom);
    const password = get(passwordAtom);

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
      await set(loginPortalAtom, credential, prelogin, () => {
        // Hide the login form after portal login success
        set(loginFormVisibleAtom, false);
      });
    } catch (err) {
      set(statusAtom, "disconnected");
      set(notifyErrorAtom, err);
    }
  }
);
