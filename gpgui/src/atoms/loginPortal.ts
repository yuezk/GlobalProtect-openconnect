import { atom } from "jotai";
import portalService, {
  PortalConfig,
  PortalCredential,
  Prelogin,
} from "../services/portalService";
import { selectedGatewayAtom } from "./gateway";
import { loginGatewayAtom } from "./loginGateway";
import { portalAddressAtom, updatePortalDataAtom } from "./portal";
import { isProcessingAtom, statusAtom } from "./status";

// Indicates whether the portal config is being fetched
// This is mainly used to show the loading indicator in the password login form
const portalConfigLoadingAtom = atom(false);

/**
 * Workflow:
 *
 * 1. Fetch portal config
 * 2. Save the portal config to the external storage
 * 3. Login the gateway, which will retrieve the token and pass it
 *    to the background service to connect the VPN
 */
export const loginPortalAtom = atom(
  (get) => get(portalConfigLoadingAtom),
  async (
    get,
    set,
    credential: PortalCredential,
    prelogin: Prelogin,
    configFetched?: () => void
  ) => {
    set(statusAtom, "portal-config");

    const portalAddress = get(portalAddressAtom);
    if (!portalAddress) {
      throw new Error("Portal is empty");
    }

    set(portalConfigLoadingAtom, true);
    let portalConfig: PortalConfig;
    try {
      portalConfig = await portalService.fetchConfig(portalAddress, credential);
      configFetched?.();
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

    // Here, we have got the portal config successfully, refresh the cached portal data
    const previousSelectedGateway = get(selectedGatewayAtom);
    const selectedGateway = gateways.find(
      ({ name }) => name === previousSelectedGateway
    );

    // Update the portal data to persist it
    await set(updatePortalDataAtom, {
      address: portalAddress,
      gateways: gateways.map(({ name, address }) => ({ name, address })),
      cachedCredential: {
        user: credential.user,
        passwd: credential.passwd,
        "portal-userauthcookie": userAuthCookie,
        "portal-prelogonuserauthcookie": prelogonUserAuthCookie,
      },
      selectedGateway: selectedGateway?.name,
    });

    // Choose the best gateway
    const { region } = prelogin;
    const { name, address } = portalService.chooseGateway(gateways, {
      region,
      preferredGateway: previousSelectedGateway,
    });

    // Log in to the gateway
    await set(loginGatewayAtom, address, {
      user: credential.user,
      userAuthCookie,
      prelogonUserAuthCookie,
    });

    // Update the selected gateway after a successful login
    await set(selectedGatewayAtom, name);
  }
);
