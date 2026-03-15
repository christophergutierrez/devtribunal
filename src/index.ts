import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadAllAgents } from "./agent-loader.js";
import { createServer } from "./server.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const agentsDir = join(__dirname, "..", "agents");

async function main() {
  const agents = await loadAllAgents(agentsDir);

  if (agents.size === 0) {
    process.stderr.write("devtribunal: no agents found in " + agentsDir + "\n");
    process.exit(1);
  }

  const specialists: string[] = [];
  const orchestrators: string[] = [];
  for (const [, agent] of agents) {
    if (agent.role === "orchestrator") {
      orchestrators.push(agent.name);
    } else {
      specialists.push(agent.name);
    }
  }

  process.stderr.write(
    `devtribunal v0.0.1\n` +
    `  ${specialists.length} specialists: ${specialists.join(", ")}\n` +
    `  ${orchestrators.length} orchestrators: ${orchestrators.join(", ")}\n` +
    `  + 2 management tools (init_agents, check_tools)\n`
  );

  const server = createServer(agents, agentsDir);
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch((err) => {
  process.stderr.write("Fatal error: " + String(err) + "\n");
  process.exit(1);
});
