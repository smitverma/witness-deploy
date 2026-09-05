import { resolve as resolvePath } from "node:path";
import { pathToFileURL } from "node:url";

const libPrefix = "$lib/";

export function resolve(specifier, context, nextResolve) {
  if (!specifier.startsWith(libPrefix)) return nextResolve(specifier, context);

  const relativePath = specifier.slice(libPrefix.length);
  const path = relativePath.endsWith(".ts") ? relativePath : `${relativePath}.ts`;
  return {
    shortCircuit: true,
    url: pathToFileURL(resolvePath(process.cwd(), "src/lib", path)).href,
  };
}
