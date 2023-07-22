import { atom } from "jotai";
import { atomWithDefault } from "jotai/utils";
import settingsService, {
  ClientOS,
  DEFAULT_SETTINGS_DATA,
  SETTINGS_DATA,
} from "../services/settingsService";
import { atomWithTauriStorage } from "../services/storageService";
import { unwrap } from "./unwrap";

const settingsDataAtom = atomWithTauriStorage(
  SETTINGS_DATA,
  DEFAULT_SETTINGS_DATA
);

const unwrappedSettingsDataAtom = atom(
  (get) => get(unwrap(settingsDataAtom)) || DEFAULT_SETTINGS_DATA
);

export const clientOSAtom = atomWithDefault<ClientOS>((get) => {
  const { clientOS } = get(unwrappedSettingsDataAtom);
  return clientOS;
});

export const osVersionAtom = atomWithDefault<string>((get) => {
  const { osVersion } = get(unwrappedSettingsDataAtom);
  return osVersion;
});

// The os version of the current OS, retrieved from the Rust backend
const currentOsVersionAtom = atomWithDefault(() =>
  settingsService.getCurrentOsVersion()
);

// The default OS version for the selected client OS
export const defaultOsVersionAtom = atomWithDefault((get) => {
  const clientOS = get(clientOSAtom);
  const osVersion = get(osVersionAtom);
  const currentOsVersion = get(unwrap(currentOsVersionAtom));

  // The current OS version is not ready, trigger the suspense,
  // to avoid the intermediate UI state
  if (!currentOsVersion) {
    return Promise.resolve("");
  }

  return settingsService.determineOsVersion(
    clientOS,
    osVersion,
    currentOsVersion
  );
});

export const clientVersionAtom = atomWithDefault<string>((get) => {
  const { clientVersion } = get(unwrappedSettingsDataAtom);
  return clientVersion;
});

export const userAgentAtom = atom((get) => {
  const clientOS = get(clientOSAtom);
  const osVersion = get(osVersionAtom);
  const currentOsVersion = get(unwrap(currentOsVersionAtom)) || "";
  const clientVersion = get(clientVersionAtom);

  return settingsService.buildUserAgent(
    clientOS,
    osVersion,
    currentOsVersion,
    clientVersion
  );
});

export const customOpenSSLAtom = atomWithDefault<boolean>((get) => {
  const { customOpenSSL } = get(unwrappedSettingsDataAtom);
  return customOpenSSL;
});

export const opensslConfigAtom = atomWithDefault(async () => {
  return settingsService.getOpenSSLConfig();
});

export const openconnectConfigAtom = atomWithDefault<string | Promise<string>>(
  () => {
    return settingsService.getOpenconnectConfig();
  }
);

export const saveSettingsAtom = atom(null, async (get, set) => {
  const clientOS = get(clientOSAtom);
  const osVersion = get(osVersionAtom);
  const clientVersion = get(clientVersionAtom);
  const customOpenSSL = get(customOpenSSLAtom);

  await set(settingsDataAtom, {
    clientOS,
    osVersion,
    clientVersion,
    customOpenSSL,
  });

  if (customOpenSSL) {
    await settingsService.updateOpenSSLConfig();
  }

  const initialOpenconnectConfig = await settingsService.getOpenconnectConfig();
  const openconnectConfig = await get(openconnectConfigAtom);

  if (initialOpenconnectConfig !== openconnectConfig) {
    await settingsService.updateOpenconnectConfig(openconnectConfig);
  }
});
