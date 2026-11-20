// A thin wrapper around the official MCP TypeScript SDK client, starting
// `rodtty mcp` over stdio exactly as an MCP host application would.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

export async function connect({ rodtty, home, name = "rodtty-e2e" }) {
  const transport = new StdioClientTransport({
    command: rodtty,
    args: ["mcp"],
    env: { ...process.env, RODTTY_HOME: home },
    stderr: "inherit",
  });
  const client = new Client({ name, version: "1.0.0" });
  await client.connect(transport);
  const call = async (tool, args = {}, options = {}) => {
    const result = await client.callTool({ name: tool, arguments: args }, undefined, {
      timeout: options.timeout ?? 120_000,
      signal: options.signal,
    });
    const value = result.structuredContent ?? JSON.parse(result.content[0].text);
    if (result.isError && !options.allowError) {
      const error = new Error(`${tool}: ${value.error?.code}: ${value.error?.message}`);
      error.code = value.error?.code;
      error.value = value;
      throw error;
    }
    return value;
  };
  return { client, transport, call, close: () => client.close() };
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function check(condition, message) {
  if (!condition) throw new Error(`check failed: ${message}`);
  console.log(`  ok  ${message}`);
}
