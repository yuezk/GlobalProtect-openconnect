import { exit } from "@tauri-apps/api/process";
import { atom } from "jotai";
import { RESET } from "jotai/utils";
import { disconnectVpnAtom } from "./gateway";
import { appDataStorageAtom, portalAddressAtom } from "./portal";
import { statusAtom } from "./status";

export const resetAtom = atom(null, (_get, set) => {
  set(appDataStorageAtom, RESET);
  set(portalAddressAtom, "");
});

export const quitAtom = atom(null, async (get, set) => {
  const status = get(statusAtom);

  if (status === "connected") {
    await set(disconnectVpnAtom);
  }
  await exit();
});
