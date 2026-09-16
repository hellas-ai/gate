import { copyFile, mkdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

const source = dirname(fileURLToPath(import.meta.url));
const output = "/tmp/hellas-gate-ui-dev";

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await copyFile(join(source, "index.html"), join(output, "index.html"));

const child = spawn(
  "esbuild",
  [
    join(source, "src/main.ts"),
    "--bundle",
    "--sourcemap",
    `--outfile=${join(output, "main.js")}`,
    `--servedir=${output}`,
    "--serve=127.0.0.1:1420",
  ],
  { stdio: "inherit" },
);

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => child.kill(signal));
}

child.on("error", (error) => {
  throw error;
});
child.on("exit", (code, signal) => {
  process.exitCode = code ?? (signal ? 128 : 1);
});
