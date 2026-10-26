import type * as T from "./types.ts";
interface TauriChannel<E> { onmessage: (message: E) => void }
interface TauriCore { invoke<R>(command: string, args?: Record<string, unknown>): Promise<R>; Channel: new <E>() => TauriChannel<E> }
declare global { interface Window { __TAURI__: { core: TauriCore } } }
const { Channel, invoke } = window.__TAURI__.core;
async function call<R>(command: string, args?: Record<string, unknown>): Promise<R> {
  try { return await invoke<R>(command, args); }
  catch (error) {
    if (typeof error === "object" && error !== null && "message" in error) throw error;
    throw { code: "internal", message: String(error) } satisfies T.ApiError;
  }
}
export const api = {
  previewApply: (document:T.DesiredState):Promise<T.ApplyPreview> => call("preview_apply",{document}),
  applyDocument: (id:string):Promise<T.ApplyReport> => call("apply_document",{id}),
  discardApply: (id:string):Promise<void> => call("discard_apply",{id}),
  quota: (id:string):Promise<T.QuotaView> => call("get_quota",{id}),
  setQuota: (input:T.QuotaInput):Promise<void> => call("set_quota",{input}),
  status: (): Promise<T.AppStatus> => call("get_status"),
  initializeSoftware: (name: string): Promise<T.AppStatus> => call("initialize_software", { name }),
  recoverSoftware: (): Promise<T.AppStatus> => call("recover_software"),
  history: (): Promise<T.HistoryEntry[]> => call("list_history", { limit: 100 }),
  deleteHistory: (id: string): Promise<boolean> => call("delete_history", { id }),
  clearHistory: (): Promise<number> => call("clear_history"),
  trust: (): Promise<T.TrustRecord[]> => call("list_trust"),
  approveTrust: (input: T.TrustInput): Promise<T.TrustRecord> => call("approve_trust", { input }),
  bindProvider: (software: boolean): Promise<T.AppStatus> => call("bind_provider", { software }),
  provider: (): Promise<T.ProviderView> => call("provider_view"),
  configureProvider: (expected: number, change: T.ProviderChange): Promise<T.ProviderView> => call("configure_provider", { expected, change }),
  resource: (expected: number, input: T.ResourceInput): Promise<T.ProviderView> => call("configure_resource", { expected, input }),
  setProvider: (enabled: boolean): Promise<T.AppStatus> => call("set_provider", { enabled }),
  contacts: (): Promise<T.ContactView[]> => call("list_contacts"),
  addContact: (input: T.ContactInput): Promise<T.ContactView> => call("add_contact", { input }),
  shares: (): Promise<T.ShareView[]> => call("list_shares"),
  share: (input: T.ShareInput): Promise<T.ShareView> => call("share_access", { input }),
  exportAccess: (id: string): Promise<string> => call("export_access", { id }),
  importAccess: (bundle: string): Promise<T.AccessView> => call("import_access", { bundle }),
  accesses: (): Promise<T.AccessView[]> => call("list_access"),
  revokeShare: (id: string): Promise<void> => call("revoke_share", { id }),
  pauseAllowance: (id: string, paused: boolean): Promise<void> => call("pause_allowance", { id, paused }),
  setGateway: (enabled: boolean, config?: T.GatewayConfig): Promise<T.AppStatus> => call("set_gateway", { enabled, config }),
  gatewayAddress: (): Promise<string> => call("gateway_address"),
  gatewayKeys: (): Promise<T.GatewayKeyView[]> => call("list_gateway_keys"),
  issueGatewayKey: (input: T.GatewayKeyInput): Promise<[T.GatewayKeyView, string]> => call("issue_gateway_key", { input }),
  issueAdminAccess: (input:T.AdminAccessInput):Promise<T.AdminAccess> => call("issue_admin_access",{input}),
  rotateGatewayKey: (old:string,input:T.GatewayKeyInput):Promise<[T.GatewayKeyView,string]> => call("rotate_gateway_key",{old,input}),
  revokeGatewayKey: (id: string): Promise<void> => call("revoke_gateway_key", { id }),
  savePaidPool: (input: T.PaidPoolInput): Promise<void> => call("save_paid_pool", { input }),
  run: async (request: T.RunRequest, onEvent: (event: T.ExecutionEvent) => void): Promise<string> => {
    const channel = new Channel<T.ExecutionEvent>(); channel.onmessage = onEvent;
    return call("run_request", { request, onEvent: channel });
  },
  cancel: (id: string): Promise<void> => call("cancel_run", { id }),
};
