import { copyFile, mkdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const source = dirname(fileURLToPath(import.meta.url));
const output = "/tmp/hellas-gate-ui-dist";

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await copyFile(join(source, "index.html"), join(output, "index.html"));

const result = spawnSync(
  "esbuild",
  [join(source, "src/main.ts"), "--bundle", "--minify", `--outfile=${join(output, "main.js")}`],
  { stdio: "inherit" },
);

if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
