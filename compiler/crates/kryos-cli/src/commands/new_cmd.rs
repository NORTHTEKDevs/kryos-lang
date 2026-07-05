//! `kryos new` — generate a new Kryos project from a template.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct NewOptions {
    /// Project name. Becomes the directory name and the `[package].name`.
    pub name: String,
    /// Template: "cli" (default), "http", "lib", or "agent".
    pub template: String,
    /// Optional explicit parent directory. Default: cwd.
    pub path: Option<String>,
}

pub fn execute(opts: NewOptions) -> Result<(), String> {
    let name = opts.name.trim();
    if name.is_empty() {
        return Err("kryos new: project name must not be empty".into());
    }
    if !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
        return Err("kryos new: project name must start with a letter or underscore".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(
            "kryos new: project name may only contain letters, digits, underscores, and hyphens"
                .into(),
        );
    }

    let parent = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("kryos new: {e}"))?,
    };
    let project_dir = parent.join(name);
    if project_dir.exists() {
        return Err(format!(
            "kryos new: '{}' already exists — refusing to overwrite",
            project_dir.display()
        ));
    }

    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("kryos new: create src dir: {e}"))?;

    write_file(&project_dir.join("kryos.toml"), &render_toml(name, &opts.template))?;
    write_file(&project_dir.join(".gitignore"), GITIGNORE)?;
    write_file(
        &project_dir.join("README.md"),
        &render_readme(name, &opts.template),
    )?;
    write_file(
        &src_dir.join("main.kry"),
        &render_main(name, &opts.template)?,
    )?;

    // Test scaffold so `kryos test` finds something out of the box.
    let tests_dir = project_dir.join("tests");
    fs::create_dir_all(&tests_dir).map_err(|e| format!("kryos new: create tests dir: {e}"))?;
    write_file(&tests_dir.join("smoke.kry"), SMOKE_TEST)?;

    eprintln!("\x1b[32mcreated\x1b[0m {} (template = {})", project_dir.display(), opts.template);
    eprintln!();
    eprintln!("next steps:");
    eprintln!("  cd {name}");
    eprintln!("  kryos run src/main.kry");
    eprintln!("  kryos test");
    eprintln!("  kryos build --release src/main.kry");

    Ok(())
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("kryos new: write {}: {e}", path.display()))
}

/// Capabilities a generated template's code declares (union of its
/// `@capabilities(...)` annotations). Mirrored into the `[capabilities]`
/// section of kryos.toml so a fresh project is strict-clean and self-documents
/// what it is allowed to do.
fn template_caps(template: &str) -> &'static [&'static str] {
    match template {
        // The HTTP scaffold binds a socket (net) and may exit(1) (process).
        "http" => &["net", "process"],
        // The agent scaffold reads env (process) and talks to an LLM API (net).
        "agent" => &["net", "process"],
        // cli, lib, and unknown templates touch no capability-gated builtins.
        _ => &[],
    }
}

fn render_toml(name: &str, template: &str) -> String {
    let allowed = template_caps(template)
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n\
         [dependencies]\n\n\
         # Capabilities this project's code is allowed to use. Mirrors the\n\
         # @capabilities(...) annotations in src/. Run\n\
         # `kryos check --strict-capabilities` to reject any function that uses a\n\
         # gated builtin (net/io/process/...) without declaring it.\n\
         [capabilities]\nallowed = [{allowed}]\n"
    )
}

fn render_readme(name: &str, template: &str) -> String {
    format!(
        "# {name}\n\nA Kryos project, generated from the `{template}` template.\n\n## Build\n\n```bash\nkryos run src/main.kry           # JIT (Cranelift)\nkryos build --release src/main.kry  # AOT (LLVM)\nkryos test                       # run tests/\n```\n\n## Layout\n\n```\n{name}/\n  kryos.toml\n  src/main.kry\n  tests/smoke.kry\n```\n"
    )
}

fn render_main(name: &str, template: &str) -> Result<String, String> {
    let body = match template {
        "cli" | "" => CLI_MAIN,
        "http" => HTTP_MAIN,
        "lib" => LIB_MAIN,
        "agent" => AGENT_MAIN,
        other => {
            return Err(format!(
                "kryos new: unknown template '{other}' — expected one of: cli, http, lib, agent"
            ))
        }
    };
    Ok(body.replace("{{NAME}}", name))
}

const CLI_MAIN: &str = "// {{NAME}} — Kryos CLI scaffold.\n//\n// This program touches no capability-gated builtins, so it declares the empty\n// capability set and passes `kryos check --strict-capabilities` as-is.\n\n@capabilities()\nfn main() {\n    let argv = args()\n    if len(argv) < 2 {\n        println(\"usage: {{NAME}} <name>\")\n        return\n    }\n    let who: str = argv[1]\n    println(\"hello, \" + who + \"!\")\n}\n";

const HTTP_MAIN: &str = "// {{NAME}} — Kryos HTTP service scaffold.
//
// Listens on 127.0.0.1:8080 and responds 200 to every request.
// Replace the handler with your own logic. The TCP functions used
// here (tcp_listen / tcp_accept / tcp_recv / tcp_send / tcp_close)
// are always-available builtins.

@capabilities(net, process)
fn main() {
    let listener = tcp_listen(\"127.0.0.1\", 8080)
    if listener < 0 {
        eprintln(\"{{NAME}}: could not bind 127.0.0.1:8080\")
        exit(1)
    }
    println(\"{{NAME}}: listening on http://127.0.0.1:8080\")
    loop {
        let conn = tcp_accept(listener)
        if conn < 0 { continue }
        let _req = tcp_recv(conn, 8192)
        let body = \"hello from {{NAME}}\n\"
        let resp = \"HTTP/1.1 200 OK\r\nContent-Length: \" + to_string(len(body)) + \"\r\nConnection: close\r\n\r\n\" + body
        tcp_send(conn, resp)
        tcp_close(conn)
    }
}
";

const LIB_MAIN: &str = "// {{NAME}} — Kryos library scaffold.\n//\n// Library crates expose public functions for consumers. Top-level\n// `main()` is optional but useful for ad-hoc testing. Neither function uses a\n// capability-gated builtin, so both declare the empty capability set and pass\n// `kryos check --strict-capabilities`.\n\n@capabilities()\npub fn greet(who: str) -> str {\n    return \"hello, \" + who\n}\n\n@capabilities()\nfn main() {\n    println(greet(\"world\"))\n}\n";

const AGENT_MAIN: &str = "// {{NAME}} — Kryos LLM agent scaffold.
//
// A budget-enforced agent on std::llm. Works against Anthropic when
// ANTHROPIC_API_KEY is set, or any OpenAI-compatible local server
// (Ollama, vLLM, LM Studio) when {{NAME}}_BASE_URL is set, e.g.:
//
//   set ANTHROPIC_API_KEY=sk-ant-...        # Claude
//   set {{NAME}}_BASE_URL=http://127.0.0.1:11434/v1   # local Ollama
//
// Run: kryos run src/main.kry \"What is the capital of Alaska?\"

use std::llm::{anthropic_config, openai_config, with_base_url, with_max_tokens, user, system, chat_within}
use std::cost::{Budget, ComputeCost, cost_add}

@capabilities(net, process)
fn main() {
    let argv = args()
    let mut prompt = \"Introduce yourself in one sentence.\"
    if len(argv) > 1 {
        prompt = argv[1]
    }

    let anthropic_key = env_get(\"ANTHROPIC_API_KEY\")
    let local_base = env_get(\"{{NAME}}_BASE_URL\")

    if anthropic_key == \"\" and local_base == \"\" {
        println(\"{{NAME}}: no provider configured (offline mode).\")
        println(\"  set ANTHROPIC_API_KEY for Claude, or\")
        println(\"  set {{NAME}}_BASE_URL for a local OpenAI-compatible server\")
        return
    }

    let mut cfg = anthropic_config(anthropic_key, \"claude-sonnet-4-6\")
    if anthropic_key == \"\" {
        cfg = with_base_url(openai_config(\"\", \"llama3\"), local_base)
    }
    cfg = with_max_tokens(cfg, 512)

    // Hard ceilings: the agent refuses to call once the budget is spent.
    let budget = Budget { max_usd: 1.0, max_tokens: 100000, max_api_calls: 20, spent_usd: 0.0, spent_tokens: 0, spent_api_calls: 0 }

    let out = chat_within(cfg, [
        system(\"You are {{NAME}}, a concise assistant.\"),
        user(prompt)
    ], budget)

    println(out.response.text)
    eprintln(\"[{{NAME}}] {out.response.input_tokens} tokens in / {out.response.output_tokens} out; budget: {out.budget.spent_tokens}/{out.budget.max_tokens} tokens, {out.budget.spent_api_calls}/{out.budget.max_api_calls} calls\")
}
";

const SMOKE_TEST: &str = "// Smoke test — exercised by `kryos test`.\n\n@test\nfn smoke_runs() {\n    let n: i64 = 2 + 2\n    assert(n == 4)\n}\n";

const GITIGNORE: &str = "# Kryos build artifacts\ntarget/\n*.o\n*.exe\n*.pdb\n*.wasm\n\n# Editor noise\n.idea/\n.vscode/\n*.swp\n.DS_Store\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kryos_new_{tag}_{}", std::process::id()))
    }

    /// Every template `kryos new` ships must produce a project that passes
    /// `kryos check --strict-capabilities` out of the box — the generated
    /// `@capabilities(...)` annotations must cover every gated builtin used.
    #[test]
    fn generated_projects_pass_strict_capabilities() {
        for template in ["cli", "http", "lib", "agent"] {
            let parent = unique_dir(template);
            let _ = std::fs::remove_dir_all(&parent);
            std::fs::create_dir_all(&parent).expect("create temp parent");

            execute(NewOptions {
                name: "demo".to_string(),
                template: template.to_string(),
                path: Some(parent.to_string_lossy().into_owned()),
            })
            .unwrap_or_else(|e| panic!("kryos new {template} failed: {e}"));

            let proj = parent.join("demo");
            // strict_capabilities = true → deny-by-default capability checking.
            let (diags, _sm) = kryos_driver::check_project_with_options(&proj, kryos_driver::CapabilityMode::Strict);
            let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
            let _ = std::fs::remove_dir_all(&parent);

            assert!(
                errors.is_empty(),
                "fresh `{template}` project must pass --strict-capabilities, got: {errors:?}"
            );
        }
    }

    /// The contrapositive: a program that calls a gated builtin from an
    /// unannotated function must FAIL strict capability checking (E0505).
    #[test]
    fn unannotated_builtin_fails_strict_capabilities() {
        use std::io::Write;
        let dir = unique_dir("leak");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let file = dir.join("leak.kry");
        {
            let mut f = std::fs::File::create(&file).expect("create temp .kry");
            writeln!(f, "fn sneaky() {{").unwrap();
            writeln!(f, "    file_write(\"out.txt\", \"data\")").unwrap();
            writeln!(f, "}}").unwrap();
            writeln!(f, "fn main() {{ sneaky() }}").unwrap();
        }

        let (diags, _sm) = kryos_driver::check_file_with_options_full(&file, false, kryos_driver::CapabilityMode::Strict);
        let has_e0505 = diags
            .iter()
            .any(|d| d.is_error() && d.code.as_deref() == Some("E0505"));
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            has_e0505,
            "unannotated file_write must fail strict check with E0505, got: {diags:?}"
        );
    }
}
