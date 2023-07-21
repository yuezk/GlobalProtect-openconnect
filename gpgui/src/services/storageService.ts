import { atom } from "jotai";
import { RESET, atomWithDefault } from "jotai/utils";
import invokeCommand from "../utils/invokeCommand";

type SetStateActionWithReset<T> =
  | T
  | typeof RESET
  | ((prev: T) => T | typeof RESET);

type KeyHint =
  | {
      key: string;
      encrypted: boolean;
    }
  | string;

type AppStorage = {
  get: <T>(key: KeyHint) => Promise<T | undefined>;
  set: <T>(key: KeyHint, value: T) => Promise<void>;
  save: () => Promise<void>;
};

export const appStorage: AppStorage = {
  get: async (key) => {
    const hint = typeof key === "string" ? { key, encrypted: false } : key;
    return invokeCommand("store_get", { hint });
  },
  set: async (key, value) => {
    const hint = typeof key === "string" ? { key, encrypted: false } : key;
    return invokeCommand("store_set", { hint, value });
  },
  save: async () => {
    return invokeCommand("store_save");
  },
};

export function atomWithTauriStorage<T>(key: KeyHint, initialValue: T) {
  const baseAtom = atomWithDefault<T | Promise<T>>(async () => {
    const storedValue = await appStorage.get<T>(key);
    if (!storedValue) {
      return initialValue;
    }
    return storedValue;
  });

  const anAtom = atom(
    (get) => get(baseAtom),
    async (get, set, update: SetStateActionWithReset<T>) => {
      const value = await get(baseAtom);
      let newValue: T | typeof RESET;
      if (typeof update === "function") {
        newValue = (update as (prev: T) => T | typeof RESET)(value);
      } else {
        newValue = update as T | typeof RESET;
      }

      if (newValue === RESET) {
        set(baseAtom, initialValue);
        await appStorage.set(key, initialValue);
      } else {
        set(baseAtom, newValue);
        await appStorage.set(key, newValue);
      }

      await appStorage.save();
    }
  );

  return anAtom;
}
