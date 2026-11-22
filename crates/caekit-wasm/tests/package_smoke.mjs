// Smoke test of the npm package as it is installed.
//
// From the repository, after build.mjs (packs crates/caekit-wasm/npm, installs the tarball
// into a temporary project and runs this script there):
//   node crates/caekit-wasm/tests/package_smoke.mjs --pack crates/caekit-wasm/npm
// In a directory where the tarball is installed (the release workflow):
//   npm install /path/to/caekit-wasm-X.Y.Z.tgz && node package_smoke.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const self = fileURLToPath(import.meta.url);
const packAt = process.argv.indexOf("--pack");

if (packAt !== -1) {
  const npmDir = path.resolve(process.argv[packAt + 1] ?? "");
  const work = mkdtempSync(path.join(tmpdir(), "caekit-wasm-smoke-"));
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  const run = (args, cwd) =>
    execFileSync(npm, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8", shell: process.platform === "win32" });
  try {
    const tarball = run(["pack", npmDir, "--pack-destination", work, "--silent"], work).trim().split("\n").pop();
    writeFileSync(path.join(work, "package.json"), JSON.stringify({ name: "smoke", private: true, type: "module" }));
    run(["install", "--no-audit", "--no-fund", "--silent", path.join(work, tarball)], work);
    copyFileSync(self, path.join(work, "package_smoke.mjs"));
    execFileSync(process.execPath, [path.join(work, "package_smoke.mjs")], { cwd: work, stdio: "inherit" });
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  process.exit(0);
}

const require = createRequire(import.meta.url);

// A 1 m bar of 100 mm² and 200 GPa under 1 kN: F L / (E A) = 0.05 mm.
const MODEL = {
  schema_version: 1,
  name: "Package smoke test",
  units: "si",
  physics: {
    kind: "axial_bar",
    geometry: {
      identity: { id: "bar", revision: "1" },
      frame: { origin_m: [0, 0, 0], direction: [1, 0, 0] },
      length_m: 1,
      cross_section_area_m2: 0.0001,
    },
    mesh: { vertices_m: [0, 0.5, 1], start_region: "clamp", end_region: "tip" },
    material: {
      id: "synthetic",
      young_modulus_pa: 200e9,
      source: { citation: "Synthetic smoke-test input", usage_terms: "Project license" },
    },
    boundary_conditions: [
      { kind: "displacement", region: "clamp", value_m: 0 },
      { kind: "force", region: "tip", value_n: 1000 },
    ],
  },
  outputs: ["end_displacement"],
};

const { version } = require("caekit-wasm/package.json");

function exercise(api, label) {
  assert.equal(api.version(), version, label);
  assert.equal(api.check(MODEL).meshes[0].cell_count, 2, label);
  const report = api.solve(JSON.stringify(MODEL));
  assert.equal(report.stop_reason, "converged", label);
  assert.equal(report.execution.backend, "wasm_cpu", label);
  // 1e-13 m: 2e-9 of the 5e-5 m extension and 8 times the algebraic error the solver threshold
  // allows for the 5-cell bar of the reference model (derivation in
  // scripts/verify-reference-cases.mjs). This 2-cell bar is also exact in P1.
  assert.ok(Math.abs(report.quantities.end_displacement.values.tip - 5e-5) <= 1e-13, label);
  assert.throws(() => api.solve({ ...MODEL, schema_version: 2 }), { name: "CaekitError", kind: "invalid_model" }, label);
}

// The package ships one module and the files a consumer needs, nothing else.
const pkgDir = path.dirname(require.resolve("caekit-wasm/package.json"));
const files = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const file = path.join(dir, name);
    if (statSync(file).isDirectory()) walk(file);
    else files.push(path.relative(pkgDir, file).split(path.sep).join("/"));
  }
};
walk(pkgDir);
assert.deepEqual(files.sort(), [
  "LICENSE-APACHE",
  "LICENSE-MIT",
  "README.md",
  "node/caekit_wasm.d.ts",
  "node/caekit_wasm.js",
  "node/caekit_wasm.mjs",
  "node/caekit_wasm_bg.wasm.d.ts",
  "node/package.json",
  "package.json",
  "web/caekit_wasm.d.ts",
  "web/caekit_wasm.js",
  "web/caekit_wasm_bg.wasm",
  "web/caekit_wasm_bg.wasm.d.ts",
]);
const wasmBytes = readFileSync(require.resolve("caekit-wasm/caekit_wasm_bg.wasm"));

// Plain import under Node: the "node" condition gives the Node build, ready on import;
// init() does nothing, so code written for the browser runs unchanged.
const root = await import("caekit-wasm");
await root.default();
root.initSync();
exercise(root, "import caekit-wasm");

// require() of the package root and of the node subpath: the same CommonJS module.
const cjs = require("caekit-wasm");
assert.equal(cjs, require("caekit-wasm/node"));
exercise(cjs, "require caekit-wasm");
assert.equal(root.solve, cjs.solve, "ESM and require share one module instance");
exercise(await import("caekit-wasm/node"), "import caekit-wasm/node");

// The web build, given the module bytes (Node cannot fetch file URLs).
const web = await import("caekit-wasm/web");
await web.default({ module_or_path: wasmBytes });
exercise(web, "web build");

console.log(
  `caekit-wasm ${version}: import, require, node and web entry points work with one ` +
    `${wasmBytes.length.toLocaleString("en-US")}-byte module`,
);
