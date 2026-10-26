const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");

const input = JSON.parse(fs.readFileSync(0, "utf8"));
const runtime = path.resolve(process.argv[2]);
const assignments = [];
const issueReads = [];
const errors = [];
const unexpectedCalls = [];
const quiet = () => {};

global.core = {
  debug: quiet,
  info: quiet,
  warning: quiet,
  error: message => errors.push(message),
  summary: { addRaw() { return this; }, async write() {} },
};
global.context = input.context;
global.github = {
  async request(route, params) {
    if (route === "GET /rate_limit") return { data: {} };
    if (route === "GET /repos/{owner}/{repo}/issues/{issue_number}/assignees/{assignee}") {
      return { data: {} };
    }
    if (route === "POST /repos/{owner}/{repo}/issues/{issue_number}/assignees") {
      assignments.push(params);
      return { data: {} };
    }
    unexpectedCalls.push(route);
    throw new Error(`Unexpected fake API request: ${route}`);
  },
  rest: {
    issues: {
      async get(params) {
        const data = {
          id: 1700,
          number: params.issue_number,
          title: "Mutable issue",
          body: input.live_body,
          state: "open",
          labels: [],
          assignees: [],
        };
        issueReads.push({ ...params, body: data.body });
        return { data };
      },
    },
  },
};
global.fetch = () => { throw new Error("Network access is forbidden"); };
global.getOctokit = () => { throw new Error("Credentials are forbidden"); };

// Permit only local runtime modules and the built-ins that the pinned handler needs.
const originalLoad = Module._load;
Module._load = function (request, parent, isMain) {
  if (["fs", "crypto", "path", "node:perf_hooks"].includes(request)) {
    return originalLoad.call(this, request, parent, isMain);
  }
  const resolved = Module._resolveFilename(request, parent);
  if (!resolved.startsWith(runtime + path.sep) || !resolved.endsWith(".cjs")) {
    throw new Error(`Module is forbidden in offline proof: ${request}`);
  }
  return originalLoad.call(this, request, parent, isMain);
};

(async () => {
  const { main } = require(path.join(runtime, "assign_to_agent.cjs"));
  const handler = await main(input.config);
  const result = await handler(input.message, {}, new Map());
  process.stdout.write(JSON.stringify({
    result, assignments, issue_reads: issueReads, errors, unexpected_calls: unexpectedCalls,
  }));
})().catch(error => {
  process.stderr.write(String(error.stack));
  process.exitCode = 1;
});
