export type Severity = "critical" | "high" | "medium" | "low" | "info";

export type Confidence = "confirmed" | "likely" | "possible";

export interface Finding {
  severity: Severity;
  confidence: Confidence;
  location: string;
  observation: string;
  why_it_matters: string;
  recommended_fix: string;
}

export type AgentRole = "specialist" | "orchestrator";

export interface AgentDefinition {
  name: string;
  description: string;
  role: AgentRole;
  languages: string[];
  severity_focus: string[];
  recommended_tools: RecommendedTool[];
  system_prompt: string;
  checklist: string;
  output_format: string;
}

export interface RecommendedTool {
  name: string;
  check: string;
  run: string;
  output_format: string;
  purpose: string;
}

export interface LinterFinding {
  tool: string;
  file: string;
  line: number | null;
  column: number | null;
  severity: string;
  message: string;
  rule: string | null;
}

export interface ReviewResult {
  agent: string;
  file: string;
  findings: Finding[];
  summary: string;
}
