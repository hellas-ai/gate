import type { ApiError, AppStatus, HistoryEntry } from "./types.ts";
interface TauriChannel<T> { onmessage: (message: T) => void }
interface TauriCore {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  Channel: new <T>() => TauriChannel<T>;
}
declare global { interface Window { __TAURI__: { core: TauriCore } } }
const { Channel, invoke } = window.__TAURI__.core;
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try { return await invoke<T>(command, args); }
  catch (error) {
    if (typeof error === "object" && error !== null && "message" in error) throw error;
    throw { code: "internal", message: String(error) } satisfies ApiError;
  }
}
export const api = {
  status: (): Promise<AppStatus> => call("get_status"),
  initializeSoftware: (name: string): Promise<AppStatus> => call("initialize_software", { name }),
  history: (): Promise<HistoryEntry[]> => call("list_history", { limit: 100 }),
  deleteHistory: (id: string): Promise<boolean> => call("delete_history", { id }),
  clearHistory: (): Promise<number> => call("clear_history"),
};
export { Channel };
