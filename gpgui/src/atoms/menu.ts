import { exit } from "@tauri-apps/api/process";
import { atom } from "jotai";
import { RESET } from "jotai/utils";
import settingsService, { TabValue } from "../services/settingsService";
import { passwordAtom, usernameAtom } from "./passwordLogin";
import { appDataAtom, portalAddressAtom } from "./portal";
import { statusAtom } from "./status";
import { disconnectVpnAtom } from "./vpn";

export const openSettingsAtom = atom(null, (_get, _set, update?: TabValue) => {
  settingsService.openSettings({ tab: update });
});

export const resetAtom = atom(null, (_get, set) => {
  set(appDataAtom, RESET);
  set(portalAddressAtom, "");
  set(usernameAtom, "");
  set(passwordAtom, "");
});

export const quitAtom = atom(null, async (get, set) => {
  const status = await get(statusAtom);

  if (status === "connected") {
    await set(disconnectVpnAtom);
  }
  await exit();
});
