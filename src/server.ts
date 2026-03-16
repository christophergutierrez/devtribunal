import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import type { AgentDefinition } from "./types.js";
import { ReviewInputSchema, reviewInputJsonSchema, executeReview } from "./tools/review.js";
import { OrchestrateInputSchema, orchestrateInputJsonSchema, executeOrchestrate } from "./tools/orchestrate.js";
import { InitInputSchema, initInputJsonSchema, executeInit } from "./tools/init.js";
import { CheckToolsInputSchema, checkToolsInputJsonSchema, executeCheckTools } from "./tools/check-tools.js";
import { loadAllAgents, resolveAgentsDir } from "./agent-loader.js";

export function createServer(
  builtinAgents: Map<string, AgentDefinition>,
  builtinAgentsDir: string,
  version: string
): Server {
  const agentCache = new Map<string, Map<string, AgentDefinition>>();

  async function getAgents(resolvedDir: string): Promise<Map<string, AgentDefinition>> {
    const cached = agentCache.get(resolvedDir);
    if (cached) return cached;
    const agents = await loadAllAgents(resolvedDir);
    agentCache.set(resolvedDir, agents);
    return agents;
  }

  const server = new Server(
    { name: "devtribunal", version },
    { capabilities: { tools: {} } }
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => {
    const tools = [];

    // Review tools — one per specialist agent
    for (const [, agent] of builtinAgents) {
      if (agent.role === "orchestrator") continue;
      tools.push({
        name: agent.name,
        description: agent.description,
        inputSchema: reviewInputJsonSchema,
      });
    }

    // Orchestrator tools — one per orchestrator agent
    for (const [, agent] of builtinAgents) {
      if (agent.role !== "orchestrator") continue;
      tools.push({
        name: agent.name,
        description: agent.description,
        inputSchema: orchestrateInputJsonSchema,
      });
    }

    // Management tools
    tools.push({
      name: "dt_init",
      description:
        "Initialize devtribunal in a target repo — scaffolds agent definitions and Claude Code skill commands",
      inputSchema: initInputJsonSchema,
    });

    tools.push({
      name: "check_tools",
      description:
        "Check which recommended linters/tools are installed for the loaded agents",
      inputSchema: checkToolsInputJsonSchema,
    });

    return { tools };
  });

  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;

    // Handle management tools
    if (name === "dt_init") {
      const parsed = InitInputSchema.safeParse(args);
      if (!parsed.success) {
        return {
          content: [
            { type: "text" as const, text: `Invalid input: ${parsed.error.message}` },
          ],
          isError: true,
        };
      }
      const result = await executeInit(parsed.data, builtinAgentsDir);
      return {
        content: [{ type: "text" as const, text: result.content }],
        isError: result.isError,
      };
    }

    if (name === "check_tools") {
      const parsed = CheckToolsInputSchema.safeParse(args);
      if (!parsed.success) {
        return {
          content: [
            { type: "text" as const, text: `Invalid input: ${parsed.error.message}` },
          ],
          isError: true,
        };
      }

      // Load agents from repo if path provided, otherwise use built-in
      let agents = builtinAgents;
      if (parsed.data.repo_path) {
        const resolvedDir = await resolveAgentsDir(
          parsed.data.repo_path,
          builtinAgentsDir,
          "directory"
        );
        if (resolvedDir !== builtinAgentsDir) {
          agents = await getAgents(resolvedDir);
        }
      }

      const result = await executeCheckTools(agents);
      return {
        content: [{ type: "text" as const, text: result.content }],
        isError: result.isError,
      };
    }

    // Check if this is an orchestrator or specialist
    const builtinAgent = builtinAgents.get(name);
    if (!builtinAgent) {
      return {
        content: [{ type: "text" as const, text: `Unknown tool: ${name}` }],
        isError: true,
      };
    }

    // Handle orchestrator tools (findings-in, not file-in)
    if (builtinAgent.role === "orchestrator") {
      const parsed = OrchestrateInputSchema.safeParse(args);
      if (!parsed.success) {
        return {
          content: [
            { type: "text" as const, text: `Invalid input: ${parsed.error.message}` },
          ],
          isError: true,
        };
      }
      const result = await executeOrchestrate(builtinAgent, parsed.data);
      return {
        content: [{ type: "text" as const, text: result.content }],
        isError: result.isError,
      };
    }

    // Handle specialist review tools — resolve agents from target file's repo
    const parsed = ReviewInputSchema.safeParse(args);
    if (!parsed.success) {
      return {
        content: [
          { type: "text" as const, text: `Invalid input: ${parsed.error.message}` },
        ],
        isError: true,
      };
    }

    // Try loading agent from target repo's devtribunal_agents/ first
    const resolvedDir = await resolveAgentsDir(
      parsed.data.file_path,
      builtinAgentsDir
    );
    let agents = builtinAgents;
    if (resolvedDir !== builtinAgentsDir) {
      agents = await getAgents(resolvedDir);
    }

    const agent = agents.get(name);
    if (!agent) {
      return {
        content: [{ type: "text" as const, text: `Unknown tool: ${name}` }],
        isError: true,
      };
    }

    const result = await executeReview(agent, parsed.data);
    return {
      content: [{ type: "text" as const, text: result.content }],
      isError: result.isError,
    };
  });

  return server;
}
