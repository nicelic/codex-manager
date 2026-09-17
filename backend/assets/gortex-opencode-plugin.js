// code-manager:gortex:opencode-plugin
// Gortex plugin for OpenCode (1.18.18).
//
// OpenCode has no lifecycle-hook configuration at all. A JS/TS plugin is
// therefore the OpenCode half of the Gortex bridge: on each relevant plugin
// hook it shells `gortex hook --agent=opencode`, writes a bridge envelope to
// stdin, and applies the decision it reads back.
//
// The file is rendered by the code-Manager adapter. The three sentinels are
// replaced with JSON literals at install time:
//
//   {{GORTEX_BIN}}       -> resolved `gortex` binary path
//   {{GORTEX_HOOK_ARGV}} -> full hook argv, argv[0] included
//   {{GORTEX_ENFORCE}}   -> boolean enforcement switch

import { execFileSync } from "node:child_process";

const GORTEX_BIN = {{GORTEX_BIN}};
const HOOK_ARGV = {{GORTEX_HOOK_ARGV}};
const ENFORCE = {{GORTEX_ENFORCE}};
const HOOK_TIMEOUT_MS = 5000;
const DECISION_CACHE_MAX = 256;

const TOOL_NAMES = {
  read: "Read",
  write: "Write",
  edit: "Edit",
  bash: "Bash",
  grep: "Grep",
  glob: "Glob",
  task: "Task",
};

const GORTEX_TOOL_PATTERN = /^(gortex[_.]|mcp__gortex__)/;

function isGortexTool(name) {
  return GORTEX_TOOL_PATTERN.test(String(name || ""));
}

function normalizeToolName(name) {
  const raw = String(name || "").trim();
  return TOOL_NAMES[raw.toLowerCase()] || raw;
}

function firstString(obj, keys) {
  if (!obj || typeof obj !== "object") return undefined;
  for (const key of keys) {
    const value = obj[key];
    if (typeof value === "string" && value !== "") return value;
  }
  return undefined;
}

function normalizeToolInput(args) {
  const out = args && typeof args === "object" ? { ...args } : {};
  const path = firstString(out, ["file_path", "filePath", "path", "file", "filepath"]);
  if (path !== undefined) out.file_path = path;
  const pattern = firstString(out, ["pattern", "glob", "query"]);
  if (pattern !== undefined) out.pattern = pattern;
  const command = firstString(out, ["command", "cmd", "script"]);
  if (command !== undefined) out.command = command;
  return out;
}

function callHook(envelope) {
  try {
    const out = execFileSync(GORTEX_BIN, HOOK_ARGV.slice(1), {
      input: JSON.stringify(envelope),
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
      timeout: HOOK_TIMEOUT_MS,
      stdio: ["pipe", "pipe", "ignore"],
    });
    const trimmed = String(out || "").trim();
    if (!trimmed) return {};
    const decision = JSON.parse(trimmed);
    return decision && typeof decision === "object" ? decision : {};
  } catch {
    return {};
  }
}

function messageText(parts) {
  if (!Array.isArray(parts)) return "";
  return parts
    .filter((part) => part && part.type === "text" && typeof part.text === "string")
    .map((part) => part.text)
    .join("\n");
}

function appendToLastTextPart(parts, text) {
  if (!Array.isArray(parts) || !text) return;
  for (let i = parts.length - 1; i >= 0; i -= 1) {
    const part = parts[i];
    if (part && part.type === "text" && typeof part.text === "string") {
      part.text = part.text + "\n\n" + text;
      return;
    }
  }
}

export const GortexPlugin = async ({ directory, worktree }) => {
  const cwd = worktree || directory || process.cwd();
  const decided = new Map();
  let oriented = false;

  function remember(callID, tip) {
    if (!callID) return;
    if (decided.size >= DECISION_CACHE_MAX) decided.clear();
    decided.set(callID, tip || "");
  }

  return {
    "tool.execute.before": async (input, output) => {
      if (!ENFORCE) return;
      const tool = String(input?.tool ?? "");
      const gortexTool = isGortexTool(tool);
      const callID = String(input?.callID ?? "");
      const decision = callHook({
        event: "tool.execute.before",
        tool_name: gortexTool ? tool : normalizeToolName(tool),
        tool_input: normalizeToolInput(output?.args),
        cwd,
        session_id: String(input?.sessionID ?? ""),
        is_gortex_tool: gortexTool,
      });

      remember(callID, decision.additional_context);
      if (decision.block) {
        throw new Error(
          decision.reason || "[Gortex] blocked - use the Gortex graph tools instead of raw file reads.",
        );
      }
    },

    "tool.execute.after": async (input, output) => {
      const callID = String(input?.callID ?? "");
      if (!callID || !decided.has(callID)) return;
      const tip = decided.get(callID);
      decided.delete(callID);
      if (!tip) return;
      try {
        if (output && typeof output.output === "string") {
          output.output = output.output + "\n\n" + tip;
        }
      } catch {
        // Context delivery is best effort and must not break the tool result.
      }
    },

    "permission.ask": async (input, output) => {
      if (!ENFORCE) return;
      const callID = String(input?.callID ?? "");
      if (callID && decided.has(callID)) return;
      const type = String(input?.type ?? "");
      if (isGortexTool(type)) return;

      const decision = callHook({
        event: "permission.ask",
        tool_name: normalizeToolName(type),
        tool_input: normalizeToolInput(input?.metadata),
        cwd,
        session_id: String(input?.sessionID ?? ""),
      });

      remember(callID, "");
      if (decision.block && output) output.status = "deny";
    },

    "chat.message": async (input, output) => {
      const role = output?.message?.role;
      if (role && role !== "user") return;
      const sessionID = String(output?.message?.sessionID ?? input?.sessionID ?? "");
      const injected = [];

      if (!oriented) {
        oriented = true;
        const briefing = callHook({ event: "session", cwd, session_id: sessionID });
        if (briefing.orientation) injected.push(briefing.orientation);
      }

      const decision = callHook({
        event: "chat.message",
        prompt: messageText(output?.parts),
        cwd,
        session_id: sessionID,
      });
      if (decision.additional_context) injected.push(decision.additional_context);

      if (injected.length > 0) {
        try {
          appendToLastTextPart(output?.parts, injected.join("\n\n"));
        } catch {
          // Message enrichment is best effort and must not break assembly.
        }
      }
    },
  };
};
