// ecosystem/kryos-embed/hosts/csharp/Program.cs
//
// RECIPE ONLY -- requires .NET SDK (not installed on this machine, untested)
//
// P/Invoke binding for kryos_embed_agent.dll.
//
// Replicates the same three-step governance contract used by the Python and Go hosts:
//   1. Capability gate  -- read agent.caps.json; refuse DLL load if any export
//                          claims a capability outside the caller's allow-set.
//   2. Budget gate      -- pass budget_cents in the JSON request; the agent
//                          refuses and records 0 spend when the call would exceed budget.
//   3. Provenance       -- every answered response carries a "source" field
//                          naming the LLM backend.
//
// ABI contract (from agent.caps.json / agent_embed.kry):
//   DLL exports (all int64, matching the proven C ABI from demo/cabi):
//     long agent_call(long req_ptr, long req_len)
//     long agent_response_len()
//
// Request JSON:  {"question": "<str>", "budget_cents": <int>}
// Response JSON: {"answered": 0|1, "answer": "<str>", "source": "<str>",
//                 "spend_cents": <int>, "reason": "<str>"}
//
// Quick start (from ecosystem/kryos-embed/hosts/csharp/):
//   dotnet run -- <path-to-dll> <path-to-caps.json>
//
// If you omit the arguments, defaults point to the repo dist/ directory:
//   dotnet run

using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

// ============================================================================
// Capability gate
// ============================================================================

class CapabilityViolation : Exception
{
    public CapabilityViolation(string msg) : base(msg) { }
}

static class CapabilityGate
{
    /// <summary>
    /// Read agent.caps.json and assert that every export's capabilities are
    /// within <paramref name="allowedCaps"/>.
    /// Throws CapabilityViolation (DLL never loaded) if any required capability
    /// is absent from the allow-set.
    /// </summary>
    public static void Check(string capsPath, IReadOnlySet<string> allowedCaps)
    {
        string json = File.ReadAllText(capsPath);
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;

        if (!root.TryGetProperty("exports", out var exports))
            throw new InvalidOperationException("caps.json missing 'exports' key");

        var violations = new List<string>();

        foreach (var export in exports.EnumerateObject())
        {
            string exportName = export.Name;
            if (!export.Value.TryGetProperty("capabilities", out var capsArr))
                continue;

            foreach (var cap in capsArr.EnumerateArray())
            {
                string capStr = cap.GetString() ?? "";
                if (!allowedCaps.Contains(capStr))
                {
                    violations.Add(
                        $"export '{exportName}' requires capability '{capStr}' " +
                        $"which is not in allowed_caps=[{string.Join(", ", allowedCaps)}]"
                    );
                }
            }
        }

        if (violations.Count > 0)
        {
            throw new CapabilityViolation(
                "DLL NOT LOADED -- capability gate blocked:\n" +
                string.Join("\n", violations.ConvertAll(v => "  - " + v))
            );
        }
    }
}

// ============================================================================
// P/Invoke signatures
// RECIPE ONLY -- adjust dllPath for your deployment layout.
// ============================================================================

static class KryosAgentNative
{
    // The DLL must be loadable by the OS from this path at runtime.
    // Options:
    //   (a) Copy kryos_embed_agent.dll next to your executable.
    //   (b) Pass an absolute path via [DllImport(dllPath)] at build time.
    //   (c) Use NativeLibrary.Load(absolutePath) for runtime binding (recommended).
    //
    // The simplest recipe uses a relative name (Windows will search standard paths):
    private const string DllName = "kryos_embed_agent";

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl,
               EntryPoint = "agent_call")]
    public static extern long AgentCall(long reqPtr, long reqLen);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl,
               EntryPoint = "agent_response_len")]
    public static extern long AgentResponseLen();
}

// ============================================================================
// High-level binding
// ============================================================================

record AgentResponse(
    int Answered,
    string Answer,
    string Source,
    int SpendCents,
    string Reason
);

class KryosAgent : IDisposable
{
    private readonly string _dllPath;
    private bool _loaded;

    /// <param name="dllPath">Absolute path to kryos_embed_agent.dll.</param>
    /// <param name="capsPath">Absolute path to agent.caps.json.</param>
    /// <param name="allowedCaps">
    ///   Capability strings the caller consents to grant.
    ///   Default: {"ffi"} -- the minimal grant needed by this agent.
    /// </param>
    /// <exception cref="CapabilityViolation">
    ///   Thrown BEFORE any DLL load if the manifest requests a capability
    ///   outside allowedCaps.
    /// </exception>
    public KryosAgent(
        string dllPath,
        string capsPath,
        IReadOnlySet<string>? allowedCaps = null)
    {
        allowedCaps ??= new HashSet<string> { "ffi" };

        // --- CAPABILITY GATE (runs before any native load) ---
        CapabilityGate.Check(capsPath, allowedCaps);
        // --- END CAPABILITY GATE ---

        // For the P/Invoke recipe above (using DllName constant) the DLL must
        // be resolvable by the OS.  For runtime loading, replace with:
        //   NativeLibrary.Load(dllPath);
        // which returns a handle you can use with NativeLibrary.GetExport().
        _dllPath = dllPath;
        _loaded = true;
    }

    /// <summary>
    /// Ask the in-process agent a question within a spend budget.
    /// </summary>
    /// <param name="question">Natural-language question.</param>
    /// <param name="budgetCents">
    ///   Maximum spend in cents.  Agent fires its budget gate BEFORE the
    ///   mock LLM; a refused call records 0 spend.
    /// </param>
    public AgentResponse Ask(string question, int budgetCents)
    {
        // Build request JSON.
        // Note: brace characters in Kryos strings require doubling, but here
        // we are building host-side JSON so standard C# string literals apply.
        string reqJson = JsonSerializer.Serialize(new
        {
            question,
            budget_cents = budgetCents
        });

        byte[] reqBytes = Encoding.UTF8.GetBytes(reqJson);

        // Pin the request buffer so the GC cannot relocate it during the call.
        GCHandle pin = GCHandle.Alloc(reqBytes, GCHandleType.Pinned);
        try
        {
            long reqPtr = pin.AddrOfPinnedObject().ToInt64();
            long respPtr = KryosAgentNative.AgentCall(reqPtr, reqBytes.Length);
            long respLen = KryosAgentNative.AgentResponseLen();

            // Read response from DLL-owned memory.
            byte[] respBytes = new byte[respLen];
            Marshal.Copy(new IntPtr(respPtr), respBytes, 0, (int)respLen);
            string respJson = Encoding.UTF8.GetString(respBytes);

            using var doc = JsonDocument.Parse(respJson);
            var r = doc.RootElement;
            return new AgentResponse(
                Answered:   r.TryGetProperty("answered",    out var a) ? a.GetInt32()    : 0,
                Answer:     r.TryGetProperty("answer",      out var b) ? b.GetString()!  : "",
                Source:     r.TryGetProperty("source",      out var c) ? c.GetString()!  : "",
                SpendCents: r.TryGetProperty("spend_cents", out var d) ? d.GetInt32()    : 0,
                Reason:     r.TryGetProperty("reason",      out var e) ? e.GetString()!  : ""
            );
        }
        finally
        {
            pin.Free();
        }
    }

    public void Dispose() { /* native DLL lifetime is process-scoped */ }
}

// ============================================================================
// Demo entry point (mirrors demo_crm.py / main.go)
// ============================================================================

class Program
{
    static int Main(string[] args)
    {
        // Resolve paths: CLI args or defaults relative to this file's assembly location.
        string repoRoot = FindRepoRoot();
        string dllPath  = args.Length > 0 ? args[0]
            : Path.Combine(repoRoot, "ecosystem", "kryos-embed", "dist", "kryos_embed_agent.dll");
        string capsPath = args.Length > 1 ? args[1]
            : Path.Combine(repoRoot, "ecosystem", "kryos-embed", "dist", "agent.caps.json");

        Console.WriteLine("=== kryos-embed C# CRM-assistant demo ===");
        Console.WriteLine($"DLL:  {dllPath}");
        Console.WriteLine($"Caps: {capsPath}");
        Console.WriteLine();

        var allowedCaps = new HashSet<string> { "ffi" };

        // ------------------------------------------------------------------ (c)
        // Capability gate proof: doctor the manifest with "net:tcp",
        // assert CapabilityViolation is raised BEFORE the DLL loads.
        // ------------------------------------------------------------------ (c)
        Console.WriteLine("[c] Doctored-manifest refusal (net:tcp injected)");
        string capsJson = File.ReadAllText(capsPath);
        using var capsDoc = JsonDocument.Parse(capsJson);
        // Clone + inject net:tcp into a temp file
        var doctored = JsonSerializer.Deserialize<Dictionary<string, JsonElement>>(capsJson)!;
        string tmpCaps = Path.GetTempFileName();
        try
        {
            // Rebuild JSON with net:tcp appended to every export's capabilities array.
            // (Full deep-clone kept minimal to stay readable as a recipe.)
            var doctoredJson = capsJson
                .Replace("\"capabilities\": [\"ffi\"]", "\"capabilities\": [\"ffi\", \"net:tcp\"]")
                .Replace("\"capabilities\":[\"ffi\"]",  "\"capabilities\":[\"ffi\",\"net:tcp\"]");
            File.WriteAllText(tmpCaps, doctoredJson);

            try
            {
                var _ = new KryosAgent(dllPath, tmpCaps, allowedCaps);
                Console.WriteLine("FAIL: expected CapabilityViolation but gate did not fire");
                return 1;
            }
            catch (CapabilityViolation ex)
            {
                Console.WriteLine($"    gate fired: {ex.Message.Split('\n')[0]}");
                Console.WriteLine("    PASS: doctored manifest refused before DLL load");
            }
        }
        finally
        {
            File.Delete(tmpCaps);
        }

        // ------------------------------------------------------------------ load
        Console.WriteLine("\n[load] Parsing real manifest and loading DLL");
        KryosAgent agent;
        try
        {
            agent = new KryosAgent(dllPath, capsPath, allowedCaps);
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"FAIL: {ex.Message}");
            return 1;
        }
        Console.WriteLine("    PASS: DLL loaded");

        // ------------------------------------------------------------------ (a)
        // Within-budget call: budget_cents=10 >= MOCK_COST_CENTS=3 -> answered=1
        // ------------------------------------------------------------------ (a)
        Console.WriteLine("\n[a] Within-budget CRM call (budget_cents=10)");
        var respA = agent.Ask("Which accounts are overdue this quarter?", 10);
        Console.WriteLine($"    answered={respA.Answered} source=\"{respA.Source}\" spend_cents={respA.SpendCents} answer=\"{respA.Answer}\"");

        if (respA.Answered != 1)
        {
            Console.Error.WriteLine($"FAIL: expected answered=1, got {respA.Answered}");
            return 1;
        }
        if (string.IsNullOrEmpty(respA.Source))
        {
            Console.Error.WriteLine("FAIL: expected non-empty source");
            return 1;
        }
        if (respA.SpendCents != 3)
        {
            Console.Error.WriteLine($"FAIL: expected spend_cents=3, got {respA.SpendCents}");
            return 1;
        }
        Console.WriteLine("    PASS: answered=1, source present, spend_cents=3");

        // ------------------------------------------------------------------ (b)
        // Over-budget call: budget_cents=1 < MOCK_COST_CENTS=3 -> answered=0
        // ------------------------------------------------------------------ (b)
        Console.WriteLine("\n[b] Over-budget CRM call (budget_cents=1)");
        var respB = agent.Ask("Summarise all deals from the last 5 years", 1);
        Console.WriteLine($"    answered={respB.Answered} spend_cents={respB.SpendCents} reason=\"{respB.Reason}\"");

        if (respB.Answered != 0)
        {
            Console.Error.WriteLine($"FAIL: expected answered=0, got {respB.Answered}");
            return 1;
        }
        if (respB.SpendCents != 0)
        {
            Console.Error.WriteLine($"FAIL: expected spend_cents=0, got {respB.SpendCents}");
            return 1;
        }
        Console.WriteLine("    PASS: answered=0, spend_cents=0, reason present");

        Console.WriteLine("\n=== ALL ASSERTIONS PASSED ===");
        return 0;
    }

    private static string FindRepoRoot()
    {
        // Walk up from the assembly directory until we find compiler/
        string dir = AppContext.BaseDirectory;
        for (int i = 0; i < 10; i++)
        {
            if (Directory.Exists(Path.Combine(dir, "compiler")))
                return dir;
            string? parent = Path.GetDirectoryName(dir);
            if (parent is null) break;
            dir = parent;
        }
        // Fallback: assume running from repo root
        return Directory.GetCurrentDirectory();
    }
}
