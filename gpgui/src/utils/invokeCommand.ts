import { invoke } from "@tauri-apps/api";

export default async function invokeCommand<T>(command: string, args?: any) {
  try {
    return await invoke<T>(command, args);
  } catch (err: any) {
    const message = err?.message ?? "Unknown error";
    throw new Error(message);
  }
}
