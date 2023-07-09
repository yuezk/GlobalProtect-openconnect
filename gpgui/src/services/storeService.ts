import { atom } from "jotai";
import { RESET, atomWithDefault } from "jotai/utils";
import { Store } from "tauri-plugin-store-api";

type SetStateActionWithReset<T> =
  | T
  | typeof RESET
  | ((prev: T) => T | typeof RESET);

export const appStore = new Store(".settings.dat");

export function atomWithTauriStorage<T>(key: string, initialValue: T) {
  const baseAtom = atomWithDefault<T | Promise<T>>(async () => {
    const storedValue = await appStore.get<T>(key);
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
        await appStore.set(key, initialValue);
      } else {
        set(baseAtom, newValue);
        await appStore.set(key, newValue);
      }

      await appStore.save();
    }
  );

  return anAtom;
}
