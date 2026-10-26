import type * as T from "./types.ts";
export const escapeHtml = (value: unknown): string => String(value).replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c] ?? c));
export const text = (data: FormData, name: string): string => String(data.get(name) ?? "").trim();
export const lines = (data: FormData, name: string): string[] => text(data, name).split(/[\n,]/).map(v => v.trim()).filter(Boolean);
export const number = (data: FormData, name: string): number => Number(text(data, name));
export const checked = (data: FormData, name: string): boolean => data.has(name);
export const field = (name: string, label: string, value = "", type = "text", required = true): string => `<label>${label}<input name="${name}" type="${type}" value="${escapeHtml(value)}" ${required ? "required" : ""}></label>`;
export const area = (name: string, label: string, value = "", required = true): string => `<label>${label}<textarea name="${name}" ${required ? "required" : ""}>${escapeHtml(value)}</textarea></label>`;
export const select = (name: string, label: string, options: Array<[string, string]>): string => `<label>${label}<select name="${name}">${options.map(([value, label]) => `<option value="${escapeHtml(value)}">${escapeHtml(label)}</option>`).join("")}</select></label>`;
export const id = (): string => Array.from(crypto.getRandomValues(new Uint8Array(16)), b => b.toString(16).padStart(2, "0")).join("");
export const requestId = (): string => `<input type="hidden" name="requestId" value="${id()}">`;
// All forms share these fixed periods; expiry conversion and overflow checks live in Rust.
const periods: Array<[T.Period, string]> = [["week", "1 week (7 days)"], ["day", "1 day"], ["month", "1 month (30 days)"], ["year", "1 year (365 days)"], ["unlimited", "Unlimited (100 years)"]];
export const periodPicker = (): string => select("period", "Access lifetime", periods);
export function period(data: FormData): T.Period { const value = text(data, "period"); if (!periods.some(([id]) => id === value)) throw new Error("Choose a fixed period"); return value as T.Period; }
export function keyInput(data: FormData): T.UpstreamKeyInput { return {service:text(data,"service"),method:text(data,"method"),key:text(data,"key"),origins:lines(data,"origins"),paths:lines(data,"paths"),methods:lines(data,"methods"),header:text(data,"header"),prefix:String(data.get("prefix") ?? ""),secret:String(data.get("secret") ?? "")}; }
export function targetFields(access: T.AccessView[], policies: T.TrustRecord[]): string {
  return select("funding", "Funding", [["authorized","Imported access"],["paid","Paid pool"]]) + select("accessId", "Imported access", access.map(a=>[a.id,`${a.label || a.id} · expires ${new Date(a.expires).toLocaleString()}`])) + field("allowance","Allowance ID (from imported access)") + field("resource","Resource name") + field("poolId","Paid pool name","","text",false) + field("provider","Paid provider transport ID","","text",false) + field("service","Paid service","","text",false) + field("method","Paid method","","text",false) + select("trustPolicy","Approved provider policy",policies.map(p=>[p.id,`${p.id} · ${p.assurance}`]));
}
export function client(data: FormData): T.WorkClientConfig {
  const target:T.WorkTarget = text(data,"funding")==="paid" ? {funding:"paid",poolId:text(data,"poolId"),provider:text(data,"provider"),service:text(data,"service"),method:text(data,"method")} : {funding:"authorized",accessId:text(data,"accessId"),allowance:text(data,"allowance"),resource:text(data,"resource")};
  return {target,trustPolicy:text(data,"trustPolicy")};
}
export async function approve(summary: string, passkey = true): Promise<boolean> {
  const dialog=document.createElement("dialog"); dialog.innerHTML=`<form method="dialog"><h2>Owner approval</h2><p></p><button value="cancel">Cancel</button><button value="approve">${passkey?"Continue to passkey":"Apply changes"}</button></form>`;
  const p=dialog.querySelector("p");if(p)p.textContent=summary;
  document.body.append(dialog);dialog.showModal();
  return new Promise(resolve=>dialog.addEventListener("close",()=>{const accepted=dialog.returnValue==="approve";dialog.remove();resolve(accepted);},{once:true}));
}
export function paidFields(): string {
  return `<details><summary>Paid funding and chain settings</summary><form id="paid-pool">`+
    field("id","Pool name")+field("provider","Provider transport ID")+area("addresses","Provider socket addresses", "",false)+field("trustPolicy","Approved trust policy ID")+field("bond","Bond ID")+area("coins","Delegated payment coin IDs")+
    field("network","Network ID")+field("genesisDigest","Genesis payload digest")+field("thresholdIdentity","Threshold finality identity (hex)")+area("validators","Six validator RPC addresses")+
    field("service","Service")+field("method","Method")+select("presentation","Fetch presentation",[["responses","Responses"],["http","HTTP"]])+field("policySalt","Agreed policy salt (hex)")+
    field("computeCreditLimit","Compute credit limit","40","number")+field("deliveryCreditLimit","Delivery credit limit","40","number")+field("fixedPrice","Price per result","10","number")+
    field("value","Payment edge value","1000","number")+field("reserve","Payment reserve","0","number")+field("omissionBond","Omission bond","601","number")+
    `<p>Funding spends use this node's live owner and actual delegation. Preview/provision is separate from saving these settings.</p><button>Save typed paid pool</button></form></details>`;
}
export function paidPool(data:FormData):T.PaidPoolInput {
  const policy:T.PaidPolicyInput={chain:{network:text(data,"network"),genesisDigest:text(data,"genesisDigest"),thresholdIdentity:text(data,"thresholdIdentity"),validators:lines(data,"validators"),committee:[],allocations:[]},service:text(data,"service"),method:text(data,"method"),presentation:text(data,"presentation")==="http"?"http":"responses",policySalt:text(data,"policySalt"),computeCreditLimit:number(data,"computeCreditLimit"),deliveryCreditLimit:number(data,"deliveryCreditLimit"),fixedPrice:number(data,"fixedPrice"),dispatchBlocks:4,deliveryBlocks:2,oracleBlocks:6,value:number(data,"value"),reserve:number(data,"reserve"),feeBase:0,feeSlot:0,feeProof:0,feeLifetime:0,minOmitResponseBlocks:8,pollMs:1000,maxObservationAgeMs:30000};
  return {id:text(data,"id"),providers:[{provider:text(data,"provider"),addresses:lines(data,"addresses"),trustPolicy:text(data,"trustPolicy"),bond:text(data,"bond"),coins:lines(data,"coins"),omissionBond:number(data,"omissionBond"),acceptanceBlocks:16,terminalBlocks:64,paymentBlocks:32,timeoutSeconds:300,policy}],maxPendingRequests:64};
}
