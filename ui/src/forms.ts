import type * as T from "./types.ts";
import { debug } from "./api.ts";
export const escapeHtml = (value: unknown): string => String(value).replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c] ?? c));
export const text = (data: FormData, name: string): string => String(data.get(name) ?? "").trim();
export const lines = (data: FormData, name: string): string[] => text(data, name).split(/[\n,]/).map(v => v.trim()).filter(Boolean);
export const number = (data: FormData, name: string): number => { const value=text(data,name);const parsed=Number(value);if(!/^\d+$/.test(value)||!Number.isSafeInteger(parsed))throw new Error(`${name}: enter a nonnegative integer within the supported numeric range`);return parsed; };
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
  return select("funding", "Funding", [["authorized","Imported access"],["paid","Paid pool"]]) + select("accessId", "Imported access", access.map(a=>[a.id,`${a.label || a.id} · expires ${new Date(a.expires).toLocaleString()}`])) + field("allowance","Allowance ID (from imported access)") + field("resource","Resource name") + field("poolId","Paid pool name","","text",false) + field("provider","Paid provider address (ENROLLMENT_ID@NODE_ID)","","text",false) + field("service","Paid service","","text",false) + field("method","Paid method","","text",false) + select("trustPolicy","Approved provider policy",policies.map(p=>[p.id,`${p.id} · ${p.assurance}`]));
}
export function client(data: FormData): T.WorkClientConfig {
  const target:T.WorkTarget = text(data,"funding")==="paid" ? {funding:"paid",poolId:text(data,"poolId"),provider:text(data,"provider"),service:text(data,"service"),method:text(data,"method")} : {funding:"authorized",accessId:text(data,"accessId"),allowance:text(data,"allowance"),resource:text(data,"resource")};
  return {target,trustPolicy:text(data,"trustPolicy")};
}
export async function approve(summary: string, passkey = true): Promise<boolean> {
  const dialog=document.createElement("dialog"); dialog.innerHTML=`<form method="dialog"><h2>Owner approval</h2><p></p><button value="cancel">Cancel</button><button value="approve">${passkey?"Continue to passkey":"Apply changes"}</button></form>`;
  const p=dialog.querySelector("p");if(p)p.textContent=summary;
  document.body.append(dialog);dialog.showModal();debug("approvalShown");
  return new Promise(resolve=>dialog.addEventListener("close",()=>{const accepted=dialog.returnValue==="approve";debug(accepted?"approvalAccepted":"approvalCancelled");dialog.remove();resolve(accepted);},{once:true}));
}
function chainFields(p?:T.PaidClientPolicyInput|T.PaidPolicyInput):string {
  return field("network","Network ID",p?.chain.network)+field("genesisDigest","Independent genesis payload digest",p?.chain.genesisDigest)+field("thresholdIdentity","Independent threshold finality identity (hex)",p?.chain.thresholdIdentity)+area("validators","Six distinct validator RPC URLs (ws:// or wss://)",p?.chain.validators.join("\n"))+
    field("service","Configured service",p?.service)+field("method","Configured method",p?.method)+select("presentation","Fetch presentation",p?.presentation==="http"?[["http","HTTP"],["responses","Responses"]]:[["responses","Responses"],["http","HTTP"]])+
    field("fixedPrice","Price per result",String(p?.fixedPrice??10),"number")+field("pollMs","Poll interval (ms)",String(p?.pollMs??1000),"number")+field("maxObservationAgeMs","Maximum progress age (ms)",String(p?.maxObservationAgeMs??30000),"number")+
    `<details><summary>Custom network genesis (omit for shipped networks)</summary>${Array.from({length:6},(_,i)=>field("committeeKey","Validator public key "+(i+1),p?.chain.committee[i]?.publicKey,"text",false)+field("committeeLabel","Validator label "+(i+1),p?.chain.committee[i]?.label,"text",false)).join("")}${area("allocationAddresses","Allocation owner addresses",p?.chain.allocations.map(a=>a.address).join("\n"),false)}${area("allocationBalances","Corresponding allocation balances",p?.chain.allocations.map(a=>a.balance).join("\n"),false)}</details>`;
}
function clientPolicy(data:FormData):T.PaidClientPolicyInput {
  const values=(name:string)=>data.getAll(name).map(String);
  const keys=values("committeeKey"),labels=values("committeeLabel"),addresses=lines(data,"allocationAddresses"),balances=lines(data,"allocationBalances");
  if(addresses.length!==balances.length)throw new Error("Provide one balance for every allocation address");
  return {chain:{network:text(data,"network"),genesisDigest:text(data,"genesisDigest"),thresholdIdentity:text(data,"thresholdIdentity"),validators:lines(data,"validators"),committee:keys.flatMap((publicKey,i)=>publicKey.trim()?[{publicKey:publicKey.trim(),label:labels[i]?.trim()??""}]:[]),allocations:addresses.map((address,i)=>({address,balance:(()=>{const n=Number(balances[i]);if(!/^\d+$/.test(balances[i]??"")||!Number.isSafeInteger(n))throw new Error("Allocation balances must be exact nonnegative integers");return n;})()}))},service:text(data,"service"),method:text(data,"method"),presentation:text(data,"presentation")==="http"?"http":"responses",fixedPrice:number(data,"fixedPrice"),pollMs:number(data,"pollMs"),maxObservationAgeMs:number(data,"maxObservationAgeMs")};
}
export function paidPolicyFields(p?:T.PaidPolicyInput, minimum=0):string {
  const f=(name:keyof T.PaidPolicyInput,label:string,fallback:number)=>field(name,label,String(p?.[name]??fallback),"number");
  const l=(name:keyof T.PaidFetchLimitsInput,label:string,fallback:number)=>field(name,label,String(p?.limits[name]??fallback),"number");
  return chainFields(p)+f("jobsPerChannel","Jobs per channel",10)+f("omissionBond","Omission collateral",101)+f("maxStakePerClient","Maximum provider stake per client",1000)+f("maxOpenOffers","Maximum open offers",16)+f("bondLifetimeBlocks","Bond lifetime (blocks)",2000)+`<label><input name="redelegatePayout" type="checkbox" ${p?.redelegatePayout===false?"":"checked"}>Keep returned stake delegated to this node</label>`+
    `<fieldset><legend>Per-job limits</legend>${l("maxRequestBodyBytes","Maximum request bytes",65536)}${l("maxOutputBytes","Maximum output bytes",4194304)}${l("maxOutputEvents","Maximum output events",4096)}${l("maxSpoolBytes","Maximum buffered bytes",8388608)}${l("maxEncodedResultFrame","Maximum result frame bytes",4194304)}${l("maxEncodedPreparedInput","Maximum prepared input bytes",131072)}${l("executionTimeoutMs","Execution timeout (ms)",300000)}</fieldset>`+
    `<details><summary>Channel timing and credit</summary>${f("computeCreditLimit","Compute credit limit",40)}${f("deliveryCreditLimit","Delivery credit limit",40)}${f("dispatchBlocks","Dispatch margin (blocks)",4)}${f("deliveryBlocks","Delivery margin (blocks)",2)}${f("oracleBlocks","Oracle grace (blocks)",6)}${f("minOmitResponseBlocks","Minimum omission-response blocks",minimum)}</details>`;
}
export function paidPolicy(data:FormData):T.PaidPolicyInput {
  return {...clientPolicy(data),computeCreditLimit:number(data,"computeCreditLimit"),deliveryCreditLimit:number(data,"deliveryCreditLimit"),dispatchBlocks:number(data,"dispatchBlocks"),deliveryBlocks:number(data,"deliveryBlocks"),oracleBlocks:number(data,"oracleBlocks"),jobsPerChannel:number(data,"jobsPerChannel"),omissionBond:number(data,"omissionBond"),maxStakePerClient:number(data,"maxStakePerClient"),maxOpenOffers:number(data,"maxOpenOffers"),bondLifetimeBlocks:number(data,"bondLifetimeBlocks"),redelegatePayout:checked(data,"redelegatePayout"),minOmitResponseBlocks:number(data,"minOmitResponseBlocks"),limits:{maxRequestBodyBytes:number(data,"maxRequestBodyBytes"),maxOutputEvents:number(data,"maxOutputEvents"),maxOutputBytes:number(data,"maxOutputBytes"),maxSpoolBytes:number(data,"maxSpoolBytes"),maxEncodedResultFrame:number(data,"maxEncodedResultFrame"),maxEncodedPreparedInput:number(data,"maxEncodedPreparedInput"),executionTimeoutMs:number(data,"executionTimeoutMs")}};
}
export function paidFields():string {
  return `<details><summary>Paid providers and chain settings</summary><form id="paid-pool">`+field("id","Pool name")+field("provider","Provider address (ENROLLMENT_ID@NODE_ID)")+area("addresses","Provider socket addresses","",false)+field("trustPolicy","Approved trust policy ID")+chainFields()+field("acceptanceBlocks","Acceptance deadline (blocks)","16","number")+field("terminalBlocks","Terminal deadline (blocks)","64","number")+field("paymentBlocks","Payment deadline (blocks)","32","number")+field("timeoutSeconds","Request timeout (seconds)","300","number")+field("maxPendingRequests","Maximum pending requests","64","number")+
    `<p>The first request asks the owner to approve exact channel funding. Each provider belongs to one pool; keep using that pool for retained work.</p><button>Save paid pool</button></form></details>`;
}
export function paidPool(data:FormData):T.PaidPoolInput {
  return {id:text(data,"id"),providers:[{provider:text(data,"provider"),addresses:lines(data,"addresses"),trustPolicy:text(data,"trustPolicy"),acceptanceBlocks:number(data,"acceptanceBlocks"),terminalBlocks:number(data,"terminalBlocks"),paymentBlocks:number(data,"paymentBlocks"),timeoutSeconds:number(data,"timeoutSeconds"),policy:clientPolicy(data)}],maxPendingRequests:number(data,"maxPendingRequests")};
}
export function paidServeFields(input:T.PaidServeInput|null, minimum=0):string {
  return `<h2>Paid Serve</h2><p>Start Serve with the matching Fetch resource, then approve its standing paid policy. The node automatically offers each client a bond within these limits. Fund the provider with owner-owned coins delegated to this node; each selected stake coin must fit the per-client cap.</p><form id="paid-serve">${paidPolicyFields(input?.policy,minimum)}<button>Approve standing paid policy</button></form>${input?'<button data-action="disable-paid-serve">Disable paid Serve and drain its work</button>':""}`;
}
export function paidServe(data:FormData):T.PaidServeInput { return {policy:paidPolicy(data)}; }
