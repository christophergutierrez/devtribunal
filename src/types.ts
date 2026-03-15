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
}

export interface RecommendedTool {
  name: string;
  check: string;
  purpose: string;
}

export interface ReviewResult {
  agent: string;
  file: string;
  findings: Finding[];
  summary: string;
}
