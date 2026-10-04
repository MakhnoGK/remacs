// Formats files with the prettier of their project, else the global one, kept loaded
// between requests. Reads one JSON request per line, { id, path, text, range? }, and
// answers each with one line: { id, text }, { id, ignored: true } or { id, error }.

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const loaded = new Map();
let globalModules;

function resolvePrettier(path) {
  try {
    return createRequire(path).resolve("prettier");
  } catch {
    globalModules ??= execFileSync("npm", ["root", "-g"], { encoding: "utf8" }).trim();
    return createRequire(join(globalModules, "noop.js")).resolve("prettier");
  }
}

function prettierFor(path) {
  const module = resolvePrettier(path);
  if (!loaded.has(module)) {
    const imported = import(pathToFileURL(module).href).then((exports) => exports.default ?? exports);
    loaded.set(module, imported);
  }
  return loaded.get(module);
}

// Prettier itself only looks in the working directory.
function ignoreFile(path) {
  for (let directory = dirname(path); ; directory = dirname(directory)) {
    const candidate = join(directory, ".prettierignore");
    if (existsSync(candidate)) return candidate;
    if (existsSync(join(directory, ".git")) || dirname(directory) === directory) return undefined;
  }
}

// Prettier loads plugins named in a config from the working directory, which is not the
// project's; resolved from the config file they are found in its node_modules.
async function resolvePlugins(prettier, path, plugins) {
  if (!plugins?.some((plugin) => typeof plugin === "string")) return plugins;
  const config = await prettier.resolveConfigFile(path);
  if (!config) return plugins;
  const require = createRequire(config);
  return plugins.map((plugin) => {
    if (typeof plugin !== "string") return plugin;
    try {
      return require.resolve(plugin);
    } catch {
      return plugin;
    }
  });
}

async function format({ path, text, range }) {
  const prettier = await prettierFor(path);
  const options = { ...(await prettier.resolveConfig(path, { editorconfig: true })), filepath: path };
  options.plugins = await resolvePlugins(prettier, path, options.plugins);
  // The plugins may add the parser for the file.
  const info = await prettier.getFileInfo(path, { ignorePath: ignoreFile(path), plugins: options.plugins });
  if (info.ignored) return { ignored: true };
  if (!info.inferredParser) throw new Error("no parser for this file");
  if (range) [options.rangeStart, options.rangeEnd] = range;
  return { text: await prettier.format(text, options) };
}

createInterface({ input: process.stdin }).on("line", async (line) => {
  const request = JSON.parse(line);
  let answer;
  try {
    answer = await format(request);
  } catch (error) {
    answer = { error: String(error?.message ?? error).split("\n")[0] };
  }
  process.stdout.write(JSON.stringify({ id: request.id, ...answer }) + "\n");
});
