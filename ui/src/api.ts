// This is the only module allowed to cross the Tauri boundary. Views consume
// typed functions and never spell command names or inspect transport errors.
import type {
  ApiError,
  AppStatus,
  ExecutionEvent,
  GatewayAccess,
  HistoryEntry,
  ProviderConfig,
  RunRequest,
} from "./types.ts";

interface TauriChannel<T> {
  onmessage: (message: T) => void;
}

interface TauriCore {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  Channel: new <T>() => TauriChannel<T>;
}

declare global {
  interface Window {
    __TAURI__: { core: TauriCore };
  }
}

const { Channel, invoke } = window.__TAURI__.core;

function normalizeError(error: unknown): ApiError {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<ApiError>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return { code: candidate.code, message: candidate.message };
    }
  }
  return { code: "internal", message: String(error) };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

export const api = {
  status: (): Promise<AppStatus> => call("get_status"),
  setProvider: (enabled: boolean, config?: ProviderConfig): Promise<AppStatus> =>
    call("set_provider_enabled", { enabled, config }),
  setGateway: (enabled: boolean, config?: RunRequest): Promise<AppStatus> =>
    call("set_gateway_enabled", { enabled, config }),
  gatewayAccess: (): Promise<GatewayAccess> => call("get_gateway_access"),
  history: (limit = 100): Promise<HistoryEntry[]> => call("list_history", { limit }),
  deleteHistory: (id: string): Promise<boolean> => call("delete_history", { id }),
  clearHistory: (): Promise<number> => call("clear_history"),
  run: async (
    request: RunRequest,
    onEvent: (event: ExecutionEvent) => void,
  ): Promise<string> => {
    const channel = new Channel<ExecutionEvent>();
    channel.onmessage = onEvent;
    return call("run_request", { request, onEvent: channel });
  },
};
