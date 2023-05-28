import { invoke } from "@tauri-apps/api";

export default async function invokeCommand<T>(command: string, args?: any) {
  try {
    return await invoke<T>(command, args);
  } catch (err: any) {
    throw new Error(err.message);
  }
}
