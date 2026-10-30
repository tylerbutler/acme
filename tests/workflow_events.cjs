const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");

const input = JSON.parse(fs.readFileSync(0, "utf8"));
const runtime = fs.realpathSync(process.argv[2]);
const calls = [];
const writes = [];
const unexpected = [];
const outputs = {};
const errors = [];
const quiet = () => {};
const issue = input.issue;
const summary = new Proxy({}, {
  get: (_, name) => name === "write" ? async () => {} : () => summary,
});
const core = {
  debug: quiet, info: quiet, warning: quiet, notice: quiet,
  error: message => errors.push(message),
  setFailed: message => errors.push(message),
  setOutput: (name, value) => { outputs[name] = value; },
  getInput: () => "", summary,
};

function forbidden(name) {
  unexpected.push(name);
  throw new Error(`Unsupported offline path: ${name}`);
}

async function api(name, params) {
  calls.push({ name, params });
  if (input.api_error === name) throw Object.assign(new Error("Fake API failure"), { status: 403 });
  const response = data => ({ data, headers: {} });
  switch (name) {
    case "repos.getCollaboratorPermissionLevel":
      return response({ permission: input.permission, user: { permissions: { admin: input.permission === "admin" } } });
    case "repos.listCollaborators":
      return response([{ login: "owner", type: "User" }]);
    case "issues.get":
      return response(structuredClone(issue));
    case "issues.listLabelsForRepo":
      return response(["needs-spec", "bug", "idea", "ready-for-agent"].map(name => ({ name })));
    case "issues.listComments":
      return response([]);
    case "issues.addLabels":
      writes.push({ name, params });
      issue.labels.push(...params.labels.map(name => ({ name })));
      return response(issue.labels);
    case "issues.createComment":
      writes.push({ name, params });
      return response({ id: 1, html_url: "https://github.com/owner/acme/issues/42#issuecomment-1" });
    case "GET /rate_limit":
    case "rateLimit.get":
      return response({});
    case "GET /repos/{owner}/{repo}/issues/{issue_number}/assignees/{assignee}":
      return response({});
    case "POST /repos/{owner}/{repo}/issues/{issue_number}/assignees":
      writes.push({ name, params });
      issue.assignees = params.assignees.map(login => ({ login }));
      return response({});
    default:
      return forbidden(name);
  }
}

const github = {
  hook: { before: quiet },
  rest: new Proxy({}, {
    get: (_, group) => new Proxy({}, {
      get: (_, method) => params => api(`${group}.${method}`, params),
    }),
  }),
  request: (route, params) => api(route, params),
  graphql: () => forbidden("graphql"),
  paginate: async (method, params) => (await method(params)).data,
};

// Block network and process modules before the pinned runtime loads.
const originalLoad = Module._load;
const isolatedFiles = path.join(process.cwd(), "runtime-files");
fs.mkdirSync(path.join(isolatedFiles, "gh-aw"), { recursive: true });
const inside = (value, root) => value === root || value.startsWith(root + path.sep);
function isolatedPath(value, write) {
  if (typeof value !== "string") return forbidden("non-string file path");
  const mapped = value === "/tmp/gh-aw" || value.startsWith("/tmp/gh-aw/")
    ? path.join(isolatedFiles, value.slice("/tmp/".length))
    : value;
  const resolved = path.resolve(mapped);
  const workspace = fs.realpathSync(process.cwd());
  if (!inside(resolved, workspace) && (write || !inside(resolved, runtime))) {
    return forbidden(`file ${value}`);
  }
  let ancestor = resolved;
  const missing = [];
  while (!fs.lstatSync(ancestor, { throwIfNoEntry: false })) {
    missing.unshift(path.basename(ancestor));
    ancestor = path.dirname(ancestor);
  }
  const real = path.resolve(fs.realpathSync(ancestor), ...missing);
  if (inside(real, workspace) || (!write && inside(real, runtime))) return resolved;
  return forbidden(`file ${value}`);
}
const reads = new Set(["existsSync", "readFileSync", "statSync", "lstatSync", "readdirSync", "realpathSync", "accessSync", "createReadStream"]);
const mutations = new Set(["writeFileSync", "appendFileSync", "mkdirSync", "rmSync", "unlinkSync", "rmdirSync", "chmodSync", "truncateSync"]);
const descriptors = new Set();
const fileSystem = new Proxy(fs, {
  get(target, name) {
    if (name === "promises") return forbidden("fs.promises");
    if (typeof target[name] !== "function") return target[name];
    if (reads.has(name) || mutations.has(name)) {
      return (file, ...args) => {
        if (name === "readFileSync" || name === "createReadStream") {
          const flags = args[0]?.[name === "readFileSync" ? "flag" : "flags"];
          if (flags !== undefined && flags !== "r" && flags !== 0) {
            return forbidden(`fs.${name} writable flags`);
          }
        }
        return target[name](isolatedPath(file, mutations.has(name)), ...args);
      };
    }
    if (name === "renameSync" || name === "copyFileSync") {
      return (source, destination, ...args) => target[name](
        isolatedPath(source, name === "renameSync"), isolatedPath(destination, true), ...args,
      );
    }
    if (name === "openSync") {
      return (file, flags, ...args) => {
        const descriptor = target.openSync(isolatedPath(file, flags !== "r" && flags !== 0), flags, ...args);
        descriptors.add(descriptor);
        return descriptor;
      };
    }
    if (name === "closeSync" || name === "fstatSync") {
      return descriptor => {
        if (!descriptors.has(descriptor)) return forbidden(`fs.${name} descriptor`);
        if (name === "closeSync") descriptors.delete(descriptor);
        return target[name](descriptor);
      };
    }
    return () => forbidden(`fs.${String(name)}`);
  },
});
Module._load = function (request, parent, isMain) {
  const builtin = request.replace(/^node:/, "");
  if (builtin === "fs") return fileSystem;
  if (["child_process", "http", "https", "net", "tls"].includes(builtin)) {
    return new Proxy({}, { get: (_, method) => () => forbidden(`${builtin}.${String(method)}`) });
  }
  if (["path", "crypto", "perf_hooks", "os", "util", "url", "assert", "buffer", "stream", "events", "async_hooks", "string_decoder", "vm", "zlib", "readline"].includes(builtin)) {
    return originalLoad.call(this, request, parent, isMain);
  }
  const resolved = Module._resolveFilename(request, parent);
  if (!resolved.startsWith(runtime + path.sep) ||
      resolved.endsWith(".test.cjs") ||
      !(resolved.endsWith(".cjs") || resolved.endsWith(".json"))) {
    return forbidden(`module ${request}`);
  }
  return originalLoad.call(this, request, parent, isMain);
};
global.fetch = () => forbidden("fetch");
const noTool = () => forbidden("external tool");
const tools = new Proxy({}, { get: () => noTool });
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

(async () => {
  for (const script of input.scripts) {
    await new AsyncFunction("core", "github", "context", "exec", "io", "getOctokit", "require", script)(
      core, github, input.context, tools, tools, () => github, require,
    );
  }
  process.stdout.write(JSON.stringify({ calls, writes, outputs, errors, unexpected, issue }));
})().catch(error => {
  process.stderr.write(error.stack);
  process.exitCode = 1;
});
