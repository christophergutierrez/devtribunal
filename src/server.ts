import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import type { AgentDefinition } from "./types.js";
import { ReviewInputSchema, executeReview } from "./tools/review.js";
import { OrchestrateInputSchema, executeOrchestrate } from "./tools/orchestrate.js";
import { InitInputSchema, executeInit } from "./tools/init.js";
import { CheckToolsInputSchema, executeCheckTools } from "./tools/check-tools.js";
import { loadAllAgents, resolveAgentsDir } from "./agent-loader.js";

const reviewInputJsonSchema = {
  type: "object" as const,
  properties: {
    file_path: {
      type: "string" as const,
      description: "Absolute path to the file to review",
    },
    context: {
      type: "string" as const,
      description: "Additional context about the file or review focus",
    },
  },
  required: ["file_path"],
};

const initInputJsonSchema = {
  type: "object" as const,
  properties: {
    repo_path: {
      type: "string" as const,
      description: "Absolute path to the target repository",
    },
    languages: {
      type: "array" as const,
      items: { type: "string" as const },
      description:
        "Languages to initialize agents for (auto-detected if omitted)",
    },
  },
  required: ["repo_path"],
};

const orchestrateInputJsonSchema = {
  type: "object" as const,
  properties: {
    findings: {
      type: "string" as const,
      description: "JSON string of review findings from specialist agents",
    },
    context: {
      type: "string" as const,
      description:
        "Additional context about the review scope or priorities",
    },
  },
  required: ["findings"],
};

const checkToolsInputJsonSchema = {
  type: "object" as const,
  properties: {
    repo_path: {
      type: "string" as const,
      description:
        "Absolute path to the repo (uses devtribunal_agents/ if present)",
    },
  },
  required: [],
};

export function createServer(
  builtinAgents: Map<string, AgentDefinition>,
  builtinAgentsDir: string
): Server {
  const agentCache = new Map<string, Map<string, AgentDefinition>>();

  async function getAgents(resolvedDir: string): Promise<Map<string, AgentDefinition>> {
    const cached = agentCache.get(resolvedDir);
    if (cached) return cached;
    const agents = await getAgents(resolvedDir);
    agentCache.set(resolvedDir, agents);
    return agents;
  }

  const server = new Server(
    { name: "devtribunal", version: "0.0.1" },
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
