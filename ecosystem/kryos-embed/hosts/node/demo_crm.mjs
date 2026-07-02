/**
 * demo_crm.mjs -- private CRM assistant demo using the Kryos WASM agent.
 *
 * Mirrors hosts/python/demo_crm.py but uses the WASM sandbox instead of the
 * native DLL.  The sandbox story is stronger: the module has NO imports beyond
 * the explicit host functions listed by kryos-agent.mjs at load time -- no
 * file system, no TCP, no WASI.  A Kryos DLL can theoretically call any
 * Windows API through an undeclared native symbol; the WASM module physically
 * cannot because every import must be resolved from the host object at
 * instantiation time.
 *
 * Demo cases:
 *   1. Within-budget call  (budget_cents=5, agent cost=3) -> answered=true, source present
 *   2. Over-budget call    (budget_cents=1, agent cost=3) -> answered=false, spendCents=0
 */

import { createAgent } from "./kryos-agent.mjs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const WASM_PATH = join(__dirname, "dist", "kryos_embed_agent.wasm");

// ---------------------------------------------------------------------------
// In-memory "CRM" -- fake data, never written to disk or sent over network.
// The wasm sandbox cannot write files or open sockets regardless.
// ---------------------------------------------------------------------------
const CUSTOMERS = {
  C001: { name: "Alice Nguyen",   plan: "Enterprise", arr_usd: 24000, since: "2023-01" },
  C002: { name: "Bob Carruthers", plan: "Pro",        arr_usd:  4800, since: "2024-03" },
  C003: { name: "Carol Ito",      plan: "Starter",    arr_usd:   600, since: "2025-11" },
};

function summariseCustomers(customers) {
  const lines = ["CRM snapshot (in-process only):"];
  for (const [cid, info] of Object.entries(customers)) {
    lines.push(`  ${cid}: ${info.name} | ${info.plan} | $${info.arr_usd.toLocaleString()}/yr | since ${info.since}`);
  }
  return lines.join("\n");
}

function assert(cond, msg) {
  if (!cond) {
    console.error("ASSERTION FAILED:", msg);
    process.exit(1);
  }
}

async function main() {
  console.log("=== Kryos Private CRM Assistant Demo (Node / WASM) ===\n");
  console.log("Security posture:");
  console.log("  - Customer data loaded in-process from a JS object (no DB, no network read)");
  console.log("  - Agent sandbox: WASM with NO imports beyond explicit host functions");
  console.log("  - Budget cap enforced by the Kryos agent BEFORE any mock-LLM call\n");

  // Load agent -- prints import manifest on load (printImports=true by default).
  // The printed manifest IS the capability proof: every function the WASM module
  // can call is enumerated here.  Anything not in the list is physically impossible.
  const agent = await createAgent(WASM_PATH, { printImports: true });
  console.log("\nAgent loaded. Import manifest printed above.\n");

  const crmCtx = summariseCustomers(CUSTOMERS);
  const question = `${crmCtx}\n\nWhich customer has the highest ARR and what plan are they on?`;

  // --- Case 1: Within-budget call (budget=5, cost=3) ---
  console.log("--- Case 1: within-budget call (budgetCents=5, agent cost=3) ---");
  const r1 = agent.ask(question, 5);
  console.log(`  answered   : ${r1.answered}`);
  console.log(`  answer     : ${JSON.stringify(r1.answer)}`);
  console.log(`  source     : ${JSON.stringify(r1.source)}`);
  console.log(`  spendCents : ${r1.spendCents}`);
  console.log(`  reason     : ${JSON.stringify(r1.reason)}`);

  assert(r1.answered === true,        `expected answered=true, got: ${JSON.stringify(r1)}`);
  assert(r1.source !== "",            `expected non-empty source (provenance): ${JSON.stringify(r1)}`);
  assert(r1.spendCents === 3,         `expected spendCents=3, got ${r1.spendCents}`);
  console.log("  PASS: answered=true, source present, spendCents=3\n");

  console.log("Data-privacy note: the CRM object above was passed as text context in the");
  console.log("question string -- it never left the JS process.  The WASM sandbox has no");
  console.log("file/network/socket imports, so exfiltration is physically impossible.\n");

  // --- Case 2: Over-budget call (budget=1, cost=3) ---
  console.log("--- Case 2: over-budget call (budgetCents=1, agent cost=3) ---");
  const r2 = agent.ask(question, 1);
  console.log(`  answered   : ${r2.answered}`);
  console.log(`  answer     : ${JSON.stringify(r2.answer)}`);
  console.log(`  spendCents : ${r2.spendCents}`);
  console.log(`  reason     : ${JSON.stringify(r2.reason)}`);

  assert(r2.answered === false,       `expected answered=false (refused), got: ${JSON.stringify(r2)}`);
  assert(r2.spendCents === 0,         `refused call must not record spend; got ${r2.spendCents}`);
  console.log("  PASS: answered=false (refusal), spendCents=0 (no charge on refusal)\n");

  console.log("=== All demo assertions PASSED ===");
  console.log("Summary:");
  console.log(`  answered=true  source='mock-llm-v1'  spendCents=3  (within-budget)`);
  console.log(`  answered=false spendCents=0                          (over-budget refusal)`);
}

main().catch((e) => { console.error(e); process.exit(1); });
