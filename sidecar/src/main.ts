/**
 * writer-agent: Writer's way to Claude, through Anthropic's Agent SDK, as
 * Edit's agent does (edit/app/apps/server/src/claude.ts). Writer (Rust)
 * starts it once per question, writes the request as one JSON line on stdin,
 * and reads one JSON line per event on stdout:
 *
 *   {"type":"reply","stopReason":"tool_use","outputTokens":812,"tools":["mcp__writer__add_section"]}
 *   {"type":"result","subtype":"success","stopReason":"end_turn","structured":{…},"costUsd":0.42,…}
 *   {"type":"error","message":"…"}
 *
 * It always runs the user's own `claude` (signed in with their plan) and
 * nothing of their setup: no settings, CLAUDE.md, hooks, MCP servers or
 * claude.ai connectors. The only tools are the ones a request names:
 * Writer's own (an HTTP MCP endpoint Rust serves, for building a script in
 * steps) and Read inside one folder (for case files and frames).
 */
import { type Options, query, type SDKMessage } from "@anthropic-ai/claude-agent-sdk";

type Request = {
  /** The user's `claude`. */
  binary: string;
  /** Where the session runs (Writer's data folder). */
  cwd: string;
  /** Writer's build, for Anthropic's client header: "nightly-9", "0.0.0-dev". */
  version: string;
  model: string;
  system: string;
  prompt: string;
  /** A structured answer in this shape (one turn, no tools unless named). */
  schema?: Record<string, unknown>;
  /** Carry on an earlier session (a repair turn). */
  resume?: string;
  /** Let Claude read files in this folder, and nothing else. */
  readDir?: string;
  /** Writer's tools for this question: an MCP server over HTTP. */
  mcp?: { name: string; url: string };
  maxTurns?: number;
};

const say = (event: Record<string, unknown>) => process.stdout.write(`${JSON.stringify(event)}\n`);

/** Edit's baseOptions: the user's own `claude`, and nothing of their setup. */
export function baseOptions(request: Request): Options {
  return {
    pathToClaudeCodeExecutable: request.binary,
    cwd: request.cwd,
    settingSources: [],
    strictMcpConfig: true,
    settings: { disableAllHooks: true },
    env: {
      ...process.env,
      ENABLE_CLAUDEAI_MCP_SERVERS: "false",
      CLAUDE_CODE_AUTO_CONNECT_IDE: "0",
      CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL: "1",
      CLAUDE_AGENT_SDK_CLIENT_APP: `writer/${request.version}`,
      // A tool call to Writer checks a plan in Rust: seconds, but never let one hang a build.
      MCP_TOOL_TIMEOUT: "300000",
    },
    stderr: () => {},
  };
}

/** The whole question's options: the base, then only what this request asks for. */
export function optionsFor(request: Request, abortController: AbortController): Options {
  const allowed = [...(request.readDir ? ["Read"] : []), ...(request.mcp ? [`mcp__${request.mcp.name}`] : [])];
  return {
    ...baseOptions(request),
    model: request.model,
    systemPrompt: request.system,
    tools: request.readDir ? ["Read"] : [],
    allowedTools: allowed,
    // Nothing prompts: what isn't allowed above is refused.
    permissionMode: "dontAsk",
    mcpServers: request.mcp ? { [request.mcp.name]: { type: "http", url: request.mcp.url } } : {},
    ...(request.readDir ? { additionalDirectories: [request.readDir] } : {}),
    ...(request.schema ? { outputFormat: { type: "json_schema", schema: request.schema } } : {}),
    ...(request.resume ? { resume: request.resume } : {}),
    ...(request.maxTurns ? { maxTurns: request.maxTurns } : {}),
    abortController,
  };
}

/** One SDK message as Writer's events: each reply's stop reason, then the result. */
export function eventsOf(message: SDKMessage): Record<string, unknown>[] {
  if (message.type === "assistant") {
    const reply = message.message as { stop_reason?: string | null; usage?: { output_tokens?: number }; content?: { type: string; name?: string }[] };
    return [
      {
        type: "reply",
        stopReason: reply.stop_reason ?? null,
        outputTokens: reply.usage?.output_tokens ?? 0,
        tools: (reply.content ?? []).filter((block) => block.type === "tool_use").map((block) => block.name ?? ""),
      },
    ];
  }
  if (message.type === "result") {
    const result = message as unknown as Record<string, unknown>;
    const usage = (result.usage ?? {}) as { output_tokens?: number };
    return [
      {
        type: "result",
        subtype: result.subtype,
        isError: result.is_error === true,
        stopReason: result.stop_reason ?? null,
        terminalReason: result.terminal_reason ?? null,
        result: typeof result.result === "string" ? result.result : null,
        errors: Array.isArray(result.errors) ? result.errors : [],
        structured: result.structured_output ?? null,
        sessionId: result.session_id ?? null,
        costUsd: typeof result.total_cost_usd === "number" ? result.total_cost_usd : 0,
        numTurns: typeof result.num_turns === "number" ? result.num_turns : 0,
        outputTokens: usage.output_tokens ?? 0,
        apiErrorStatus: result.api_error_status ?? null,
      },
    ];
  }
  return [];
}

async function main() {
  const text = await Bun.stdin.text();
  let request: Request;
  try {
    request = JSON.parse(text) as Request;
  } catch {
    say({ type: "error", message: "writer-agent got a request it can't read." });
    process.exit(2);
  }
  const abort = new AbortController();
  // Writer stopping the build (or quitting) ends the question and Claude Code with it.
  for (const signal of ["SIGTERM", "SIGINT"] as const) process.on(signal, () => abort.abort());
  try {
    const answer = query({ prompt: request.prompt, options: optionsFor(request, abort) });
    for await (const message of answer) for (const event of eventsOf(message)) say(event);
  } catch (error) {
    say({ type: "error", message: error instanceof Error ? error.message : String(error) });
    process.exit(1);
  }
}

if (import.meta.main) await main();
