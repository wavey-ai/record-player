import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const root = resolve(repoRoot, "dist");
const vendorRoot = resolve(repoRoot, "..", "vin.yl.vendor");
const port = Number(process.env.PORT || process.argv[2] || 5193);
const types = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".cjs", "text/javascript; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".wasm", "application/wasm"],
  [".onnx", "application/octet-stream"],
  [".data", "application/octet-stream"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"]
]);

createServer((request, response) => {
  const pathname = new URL(request.url, "http://localhost").pathname;
  const relative = pathname === "/" ? "index.html" : pathname.slice(1);
  const candidate = normalize(join(root, relative));
  if (!candidate.startsWith(root)) {
    response.writeHead(403).end();
    return;
  }

  try {
    const path = (relative.startsWith("wasm")) ? join(vendorRoot, relative) : candidate;
    const info = statSync(path);
    if (!info.isFile()) throw new Error("not a file");
    response.writeHead(200, {
      "Content-Type": types.get(extname(path)) || "application/octet-stream",
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp"
    });
    createReadStream(path).pipe(response);
  } catch (error) {
    if (error?.code !== "ENOENT") console.error(error);
    response.writeHead(404).end("Not found");
  }
}).listen(port, () => {
  console.log(`http://localhost:${port}`);
});
