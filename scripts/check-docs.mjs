#!/usr/bin/env node
// Fails when the documentation has drifted from the code it describes.
//
//   node scripts/check-docs.mjs
//
// AGENTS.md lists what a change has to update. This script checks the parts of that
// contract that a program can check: every physics kind has its example, page, tests and
// README row; every output is documented; every crate is described; versions quoted in
// prose match the manifests; links resolve; published READMEs use absolute links; every page
// with Rust examples is compiled by docs/doctests.rs; MUST statuses match the specification
// summary; test tolerances state their reason; every row of the provenance methods table has
// an identifier and a test, and every reported algorithm has a row; and every refinement study
// on a physics page names a command that runs existing tests. Only Node.js built-ins are used.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const problems = [];
const problem = (message) => problems.push(message);
const exists = (file) => existsSync(path.join(root, file));
const missing = new Set();
/** The text of a file, or an empty string and one reported problem when it is missing. */
function read(file) {
  if (!exists(file)) {
    if (!missing.has(file)) problem(`${file} is missing`);
    missing.add(file);
    return "";
  }
  return readFileSync(path.join(root, file), "utf8").replaceAll("\r\n", "\n");
}

function requireFile(file, why) {
  if (!exists(file) && !missing.has(file)) {
    missing.add(file);
    problem(`${file} is missing (${why})`);
  }
  return exists(file);
}

const snake = (name) => name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();
const kebab = (name) => snake(name).replaceAll("_", "-");

/** Variant names of `pub enum <name>` in a Rust source file. */
function enumVariants(file, name) {
  const match = new RegExp(`pub enum ${name} \\{\\n([\\s\\S]*?)\\n\\}`).exec(read(file));
  if (!match) {
    problem(`${file}: pub enum ${name} not found; update scripts/check-docs.mjs`);
    return [];
  }
  return [...match[1].matchAll(/^ {4}([A-Z][A-Za-z0-9]*)\b/gm)].map((variant) => variant[1]);
}

// ---- Markdown files of the repository --------------------------------------------------
// Build output, and every hidden directory except .github.
const SKIP_DIRS = new Set(["target", "node_modules", "npm", "pkg", "dist", "runs"]);
const skipped = (entry) => SKIP_DIRS.has(entry) || (entry.startsWith(".") && entry !== ".github");
function markdownFiles(dir = "") {
  const found = [];
  for (const entry of readdirSync(path.join(root, dir))) {
    const file = path.posix.join(dir, entry);
    if (statSync(path.join(root, file)).isDirectory()) {
      if (!skipped(entry)) found.push(...markdownFiles(file));
    } else if (entry.endsWith(".md")) {
      found.push(file);
    }
  }
  return found.sort();
}
const pages = markdownFiles();
const prose = (file) => read(file).replace(/^```[\s\S]*?^```/gm, "");

// ---- Physics kinds ---------------------------------------------------------------------
const kinds = enumVariants("crates/caekit/src/model.rs", "Physics");
if (kinds.length === 0) problem("crates/caekit/src/model.rs: enum Physics has no variants");
const readme = read("README.md");
const references = read("scripts/verify-reference-cases.mjs");
const cliTests = read("crates/caekit-cli/tests/reference_cases.rs");
for (const variant of kinds) {
  const kind = snake(variant);
  const page = `docs/physics/${kebab(variant)}.md`;
  const example = `examples/${kind}.json`;
  if (requireFile(example, `reference model of physics kind ${kind}`)) {
    const model = JSON.parse(read(example));
    if (model.physics?.kind !== kind) problem(`${example}: physics.kind is ${model.physics?.kind}, expected ${kind}`);
  }
  if (requireFile(page, `physics page of kind ${kind}`)) {
    if (!read(page).includes(`\`${kind}\``) && !read(page).includes(`"${kind}"`)) problem(`${page} never names the kind \`${kind}\``);
    if (!read(page).includes(example)) problem(`${page} does not show how to run ${example}`);
  }
  requireFile(`crates/caekit/tests/${kind}.rs`, `library tests of physics kind ${kind}`);
  if (!read("crates/caekit/tests/model_identity.rs").includes(`"../../../examples/${kind}.json"`)) {
    problem(`crates/caekit/tests/model_identity.rs does not freeze the digest of ${example}`);
  }
  if (!readme.includes(`(${page})`) || !readme.includes(`\`${kind}\``)) {
    problem(`README.md: the physics table has no row for \`${kind}\` that links ${page}`);
  }
  if (!new RegExp(`^  ${kind}\\(report, label\\) \\{`, "m").test(references)) {
    problem(`scripts/verify-reference-cases.mjs has no analytical reference for ${kind}`);
  }
  if (!cliTests.includes(`check_and_run("${kind}.json")`)) {
    problem(`crates/caekit-cli/tests/reference_cases.rs does not run ${kind}.json`);
  }
  const crateReadme = read("crates/caekit/README.md");
  if (!crateReadme.includes(`\`${kind}\``)) problem(`crates/caekit/README.md: the physics table has no row for \`${kind}\``);
}
const known = new Set(kinds.map(snake));
for (const entry of readdirSync(path.join(root, "examples"))) {
  if (entry.endsWith(".json") && !known.has(path.basename(entry, ".json"))) problem(`examples/${entry} matches no physics kind`);
}
for (const entry of readdirSync(path.join(root, "docs", "physics"))) {
  if (entry.endsWith(".md") && !known.has(path.basename(entry, ".md").replaceAll("-", "_"))) {
    problem(`docs/physics/${entry} matches no physics kind`);
  }
}

// ---- Outputs ---------------------------------------------------------------------------
const modelPage = read("docs/model.md");
for (const variant of enumVariants("crates/caekit-types/src/quantity.rs", "Output")) {
  if (!modelPage.includes(`\`${snake(variant)}\``)) problem(`docs/model.md does not describe the output \`${snake(variant)}\``);
}

// ---- Requirement and extension review -------------------------------------------------
const contributorRules = read("AGENTS.md");
for (const section of ["choosing-the-next-requirement", "definition-of-done", "spec-ext-005"]) {
  if (!contributorRules.includes(`(docs/design/spec.md#${section})`)) {
    problem(`AGENTS.md must link the design specification section #${section}`);
  }
}
if (!/^\| A requirement of .*?docs\/design\/spec\.md.*?\|[^\n]*Status[^\n]*summary[^\n]*commit body/m.test(contributorRules)) {
  problem("AGENTS.md must require updates to the requirement status and MUST summary and an ID in the commit body");
}
const specification = read("docs/design/spec.md");
const mustStatus = new Map(
  specification.split(/^### /m).flatMap((section) => {
    const entry = /^(SPEC-[A-Z]+-\d+)\n[\s\S]*?^- Priority: MUST\b[^\n]*(?:\n {2}[^\n]*)*\n- Status: (Implemented|Partial|Planned)\b/m.exec(section);
    return entry ? [[entry[1], entry[2]]] : [];
  }),
);
const mustRows = [...specification.matchAll(/^\| \[(SPEC-[A-Z]+-\d+)\]\(#[^)]*\) \|[^\n]+\| (Implemented|Partial|Planned) \|$/gm)];
const summaryStatus = new Map(mustRows.map(([, id, status]) => [id, status]));
if (summaryStatus.size !== mustRows.length) problem("docs/design/spec.md: duplicate MUST summary rows");
for (const id of mustStatus.keys()) {
  if (!summaryStatus.has(id)) problem(`docs/design/spec.md: ${id} is missing from the MUST summary`);
}
for (const [, id, status] of mustRows) {
  if (mustStatus.get(id) !== status) {
    problem(`docs/design/spec.md: ${id} has MUST status ${mustStatus.get(id)} but its summary says ${status}`);
  }
}

// ---- Workspace crates ------------------------------------------------------------------
const manifest = read("Cargo.toml");
const members = [...(/^members = \[\n([\s\S]*?)\n\]/m.exec(manifest)?.[1] ?? "").matchAll(/"([^"]+)"/g)].map((member) => member[1]);
if (members.length === 0) problem("Cargo.toml: workspace members not found; update scripts/check-docs.mjs");
const architecture = read("docs/ARCHITECTURE.md");
const publishedReadmes = [];
for (const member of members.filter((member) => member.startsWith("crates/"))) {
  const name = path.posix.basename(member);
  const crateReadme = `${member}/README.md`;
  if (requireFile(crateReadme, `README of the package ${name}`)) publishedReadmes.push(crateReadme);
  if (!readme.includes(`\`${name}\``)) problem(`README.md: the workspace table does not list \`${name}\``);
  if (!architecture.includes(`\`${name}\``)) problem(`docs/ARCHITECTURE.md does not describe \`${name}\``);
}

// ---- Versions quoted in prose ----------------------------------------------------------
const msrv = /^rust-version = "([^"]+)"/m.exec(manifest)?.[1];
if (!msrv) problem("Cargo.toml: rust-version not found");
const workflow = read(".github/workflows/verify.yml");
const nodeVersions = new Set([...workflow.matchAll(/node-version: "(\d+)"/g)].map((match) => match[1]));
if (nodeVersions.size !== 1) problem(`.github/workflows/verify.yml uses several Node.js versions: ${[...nodeVersions].join(", ")}`);
const [node] = nodeVersions;
for (const page of pages) {
  const text = read(page);
  for (const [, quoted] of text.matchAll(/\bRust (\d+\.\d+)(?:\.\d+)?\b(?! \()/g)) {
    // A page may name the toolchain a measurement was taken with; that is not the MSRV.
    if (quoted !== msrv && !/\bwith Ubuntu\b|\bmeasured\b/i.test(text.slice(Math.max(0, text.indexOf(`Rust ${quoted}`) - 200), text.indexOf(`Rust ${quoted}`) + 200))) {
      problem(`${page} names Rust ${quoted}; Cargo.toml declares rust-version ${msrv}`);
    }
  }
  for (const [, quoted] of text.matchAll(/cargo \+(\d+\.\d+)\.\d+ /g)) {
    if (quoted !== msrv) problem(`${page} runs cargo +${quoted}; Cargo.toml declares rust-version ${msrv}`);
  }
  for (const [, quoted] of text.matchAll(/Node\.js (\d+)/g)) {
    if (quoted !== node && !/\bNode\.js \d+ or newer\b/.test(text)) problem(`${page} names Node.js ${quoted}; CI uses ${node}`);
  }
}

// ---- Links -----------------------------------------------------------------------------
// GitHub heading anchors: lower case, punctuation dropped, spaces to hyphens.
function anchors(file) {
  const seen = new Map();
  const result = new Set();
  for (const [, heading] of prose(file).matchAll(/^#{1,6} +(.+?) *#*$/gm)) {
    const base = heading
      .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/[`*_]/g, "")
      .toLowerCase()
      .replace(/[^\p{L}\p{N} _-]/gu, "")
      .replaceAll(" ", "-");
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    result.add(count === 0 ? base : `${base}-${count}`);
  }
  return result;
}
for (const page of pages) {
  for (const [, target] of prose(page).matchAll(/\[[^\]]*\]\(([^)\s]+)(?: "[^"]*")?\)/g)) {
    if (/^(https?:|mailto:)/.test(target)) continue;
    if (publishedReadmes.includes(page)) {
      problem(`${page} links ${target}: a README published to a registry must use absolute URLs`);
      continue;
    }
    const [file, anchor] = target.split("#");
    const resolved = file === "" ? page : path.posix.normalize(path.posix.join(path.posix.dirname(page), file));
    if (resolved.startsWith("..") || !exists(resolved)) {
      problem(`${page} links ${target}, which does not exist`);
    } else if (anchor && resolved.endsWith(".md") && !anchors(resolved).has(anchor)) {
      problem(`${page} links ${target}: ${resolved} has no heading with the anchor #${anchor}`);
    }
  }
}

// ---- Rust examples are compiled --------------------------------------------------------
const doctests = read("docs/doctests.rs");
for (const page of pages) {
  if (!/^```rust/m.test(read(page))) continue;
  // The README of the caekit crate is its crate documentation and is tested there.
  if (page === "crates/caekit/README.md") {
    if (!read("crates/caekit/src/lib.rs").includes('include_str!("../README.md")')) {
      problem("crates/caekit/src/lib.rs must include ../README.md as the crate documentation");
    }
    continue;
  }
  const include = path.posix.relative("docs", page);
  if (!doctests.includes(`include_str!("${include}")`)) {
    problem(`${page} has Rust examples that nothing compiles: add include_str!("${include}") to docs/doctests.rs`);
  }
}
for (const [, include] of doctests.matchAll(/include_str!\("([^"]+)"\)/g)) {
  if (!exists(path.posix.normalize(path.posix.join("docs", include)))) problem(`docs/doctests.rs includes ${include}, which does not exist`);
}

// ---- Tolerances state their reason (SPEC-EVD-002) ---------------------------------------
// Scope, kept narrow so that a report is always a real gap:
// - Rust tests in crates/caekit/tests (with its common/ modules) and crates/caekit-cli/tests:
//   the third argument of `close(` and `close_tensor(`, and the value assigned to a
//   `*_residual_tolerance` solver setting. A bare numeric literal there needs a `//` comment on
//   its line, on the first line of the call or on the line above; every UPPER_CASE constant it
//   names must be declared in the file or in common/ with a comment directly above it (or above
//   the first constant of its run of consecutive declarations). Every constant whose name says
//   it is a tolerance (TOL, ALLOWANCE, BAND, BOUND, MARGIN, BUDGET, RANGE, FLOOR, ROUNDING,
//   EXACT) needs a comment directly above its own declaration, and so does any comparison of a
//   value with a literal of the form `<n>e-<k>` outside a constant declaration.
// - scripts/verify-reference-cases.mjs: a numeric literal as the third argument of `close(`,
//   `compareNumbers(` or `compareNamedNumbers(`, each unit floor of the `absoluteTolerance` map and each literal tolerance
//   of a `[name, unit, tolerance]` row need a `//` comment on their line or the line above (or
//   above the call). Library code is outside this check: its public items are documented by the
//   `missing_docs` lint, and its private constants were reviewed by hand.
const isComment = (line) => /^\s*\/\//.test(line ?? "");
const isAttribute = (line) => /^\s*#\[/.test(line ?? "");
const numericLiteral = /^-?\d[\d_]*(?:\.\d[\d_]*)?(?:e[+-]?\d+)?(?:_?f64)?$/;
const containsLiteral = (text) => /(?<![\w.])\d[\d_]*(?:\.\d[\d_]*)?(?:e[+-]?\d+)?(?![\w.])/.test(text);
/** The text between the parenthesis at `open` and its partner, and the index after it. */
function balanced(text, open) {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    if ("([{".includes(text[i])) depth += 1;
    if (")]}".includes(text[i])) depth -= 1;
    if (depth === 0) return [text.slice(open + 1, i), i + 1];
  }
  return [text.slice(open + 1), text.length];
}
/** Top-level comma-separated arguments with their offsets inside `body`. */
function splitArguments(body) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i <= body.length; i += 1) {
    const c = body[i];
    if (c !== undefined && "([{".includes(c)) depth += 1;
    if (c !== undefined && ")]}".includes(c)) depth -= 1;
    if (i === body.length || (c === "," && depth === 0)) {
      const raw = body.slice(start, i);
      const lead = raw.length - raw.trimStart().length;
      parts.push({ text: raw.trim(), offset: start + lead });
      start = i + 1;
    }
  }
  return parts.filter((part) => part.text !== "");
}
const lineAt = (text, index) => text.slice(0, index).split("\n").length - 1;
/** Code without line comments and string contents, line for line. */
const stripCode = (text) =>
  text
    .split("\n")
    .map((line) => line.replace(/"(?:[^"\\]|\\.)*"/g, '""').replace(/`(?:[^`\\]|\\.)*`/g, "``").replace(/\/\/.*$/, ""))
    .join("\n");
function rustFiles(dir) {
  if (!exists(dir)) return [];
  return readdirSync(path.join(root, dir)).flatMap((entry) => {
    const file = path.posix.join(dir, entry);
    if (statSync(path.join(root, file)).isDirectory()) return rustFiles(file);
    return entry.endsWith(".rs") ? [file] : [];
  });
}
const TOLERANCE_NAME = /(?:^|_)(?:TOL|TOLERANCE|ALLOWANCE|BAND|BOUND|MARGIN|BUDGET|RANGE|FLOOR|ROUNDING|EXACT)(?:_|$)/;
/** Constants declared in `lines`, with whether a comment documents them. */
function declaredConstants(lines) {
  const constants = new Map();
  let runDocumented = false;
  let previousWasConst = false;
  lines.forEach((line, index) => {
    const declaration = /^\s*(?:pub(?:\([a-z]+\))? )?const ([A-Z][A-Z0-9_]*)\s*:/.exec(line);
    if (!declaration) {
      // A multi-line declaration keeps its run open until the line that ends it.
      if (previousWasConst && !/;\s*$/.test(lines[index - 1] ?? "")) return;
      previousWasConst = false;
      return;
    }
    let above = index - 1;
    while (isAttribute(lines[above])) above -= 1;
    const commented = isComment(lines[above]);
    const startsRun = !previousWasConst || commented;
    if (startsRun) runDocumented = commented;
    constants.set(declaration[1], { line: index, documented: commented || runDocumented, own: commented });
    previousWasConst = true;
  });
  return constants;
}
const toleranceRoots = ["crates/caekit/tests", "crates/caekit-cli/tests"];
for (const testRoot of toleranceRoots) {
  const files = rustFiles(testRoot);
  const shared = new Map();
  for (const file of files.filter((file) => file.includes("/common/"))) {
    for (const [name, entry] of declaredConstants(read(file).split("\n"))) shared.set(name, { ...entry, file });
  }
  for (const file of files) {
    const text = read(file);
    const lines = text.split("\n");
    const own = new Map([...declaredConstants(lines)].map(([name, entry]) => [name, { ...entry, file }]));
    const known = new Map([...shared, ...own]);
    for (const [name, entry] of own) {
      if (TOLERANCE_NAME.test(name) && !entry.own) {
        problem(`${file}:${entry.line + 1}: tolerance constant ${name} needs a comment that states its reason`);
      }
    }
    const commentedNear = (literalLine, callLine) =>
      [literalLine, callLine].some((line) => /\/\//.test(lines[line] ?? "")) || isComment(lines[callLine - 1]);
    const checkTolerance = (expression, literalLine, callLine, what) => {
      if (numericLiteral.test(expression)) {
        if (!commentedNear(literalLine, callLine)) problem(`${file}:${literalLine + 1}: ${what} ${expression} needs a named constant or a comment with its reason`);
        return;
      }
      for (const [, name] of expression.matchAll(/(?<![\w:])([A-Z][A-Z0-9_]*[A-Z0-9])(?![\w(])/g)) {
        if (/^[A-Z]$/.test(name)) continue;
        const entry = known.get(name);
        if (entry && !entry.documented) problem(`${entry.file}:${entry.line + 1}: ${name}, used as ${what} in ${file}:${literalLine + 1}, needs a comment that states its reason`);
      }
      if (!/[A-Za-z_]/.test(expression) && containsLiteral(expression) && !commentedNear(literalLine, callLine)) {
        problem(`${file}:${literalLine + 1}: ${what} ${expression} needs a named constant or a comment with its reason`);
      }
    };
    for (const call of text.matchAll(/(?<![\w.])(close|close_tensor)\(/g)) {
      if (/\bfn\s+$/.test(text.slice(Math.max(0, call.index - 4), call.index))) continue;
      const open = call.index + call[0].length - 1;
      const [body] = balanced(text, open);
      const tolerance = splitArguments(body)[2];
      if (!tolerance) continue;
      checkTolerance(tolerance.text, lineAt(text, open + 1 + tolerance.offset), lineAt(text, call.index), "the tolerance");
    }
    for (const setting of text.matchAll(/\b[a-z_]*residual_tolerance(?:_[a-z]+)?\s*=\s*([^;]+);/g)) {
      checkTolerance(setting[1].trim(), lineAt(text, setting.index), lineAt(text, setting.index), "the solver tolerance");
    }
    const code = stripCode(text).split("\n");
    code.forEach((line, index) => {
      if (/\bconst [A-Z]/.test(line)) return;
      const comparison = /(?:<=?|>=?)\s*(\d+(?:\.\d+)?e-\d+)\b/.exec(line);
      if (comparison && !/\/\//.test(lines[index]) && !isComment(lines[index - 1])) {
        problem(`${file}:${index + 1}: the comparison with ${comparison[1]} needs a named constant or a comment with its reason`);
      }
    });
  }
}
{
  const file = "scripts/verify-reference-cases.mjs";
  const text = read(file);
  const lines = text.split("\n");
  const commentedAt = (line) => /\/\//.test(lines[line] ?? "") || isComment(lines[line - 1]);
  for (const call of text.matchAll(/(?<![\w.])(close|compareNumbers|compareNamedNumbers)\(/g)) {
    if (/function\s+$/.test(text.slice(Math.max(0, call.index - 9), call.index))) continue;
    const open = call.index + call[0].length - 1;
    const [body] = balanced(text, open);
    const tolerance = splitArguments(body)[2];
    if (!tolerance || !numericLiteral.test(tolerance.text)) continue;
    const literalLine = lineAt(text, open + 1 + tolerance.offset);
    const callLine = lineAt(text, call.index);
    if (!commentedAt(literalLine) && !commentedAt(callLine)) {
      problem(`${file}:${literalLine + 1}: the tolerance ${tolerance.text} of ${call[1]}() needs a comment with its reason`);
    }
  }
  const floors = /const absoluteTolerance = new Map\(\[\n([\s\S]*?)\n\]\);/.exec(text);
  if (!floors) problem(`${file}: the absoluteTolerance map was not found; update scripts/check-docs.mjs`);
  else {
    const first = lineAt(text, floors.index) + 1;
    floors[1].split("\n").forEach((line, offset) => {
      const entry = /^\s*\["([^"]+)",\s*([^\]]+)\]/.exec(line);
      if (entry && !commentedAt(first + offset)) problem(`${file}:${first + offset + 1}: the floor of unit ${entry[1]} needs a comment with its reason`);
    });
  }
  lines.forEach((line, index) => {
    const row = /\[\s*`?[^,\]]*`?,\s*"[^"]+",\s*(-?\d[\d_.]*(?:e[+-]?\d+)?)\s*\]/.exec(line.replace(/\/\/.*$/, ""));
    if (row && !line.includes("new Map") && !commentedAt(index)) problem(`${file}:${index + 1}: the tolerance ${row[1]} needs a comment with its reason`);
  });
}

// ---- Methods table of the provenance page (SPEC-EVD-008, SPEC-EXT-009) -----------------
// Every row of the methods table names a resolvable identifier (a DOI, an ISBN, a standard or
// an https URL) and a verifying test, and every algorithm identifier that the solvers report
// is a row of the stage table in docs/model.md and the first column of a methods row.
{
  const file = "docs/provenance.md";
  const section = /^## Methods and cited sources\n([\s\S]*?)(?=^## |(?![\s\S]))/m.exec(read(file));
  const header = "| Method | Where used | Source | Identifier | Verifying test |";
  const rows = [];
  if (!section || !section[1].includes(header)) {
    problem(`${file} needs a "Methods and cited sources" section with the table header ${header}`);
  } else {
    const table = section[1].slice(section[1].indexOf(header)).split("\n").slice(2);
    for (const line of table) {
      if (!line.startsWith("|")) break;
      const cells = line.slice(1, -1).split(/(?<!\\)\|(?![^`]*`[^`|]*\|)/).map((cell) => cell.trim());
      rows.push(cells);
      const [method, , , identifier = "", test = ""] = cells;
      if (!/doi:\[10\.\d{4,9}\/[^\]]+\]\(https:\/\/doi\.org\/|ISBN [\d-]{10,17}|\]\(https:\/\/[^)]+\)/.test(identifier)) {
        problem(`${file}: the methods row "${method.slice(0, 60)}" needs a DOI, an ISBN or an https link as its identifier`);
      }
      if (test === "" || test === "—" || test === "-") problem(`${file}: the methods row "${method.slice(0, 60)}" names no verifying test`);
    }
  }
  const stageTable = /^\| Stage \| Physics kinds \| Residual unit \| Algorithm \| Settings \|\n\|[-| ]+\|\n((?:\|.*\|\n?)+)/m.exec(modelPage);
  const stageAlgorithms = new Set();
  if (!stageTable) problem("docs/model.md: the stage table with an Algorithm column was not found; update scripts/check-docs.mjs");
  else {
    for (const line of stageTable[1].trim().split("\n")) {
      const algorithm = /`([a-z0-9_]+)`/.exec(line.split("|")[4] ?? "");
      if (algorithm) stageAlgorithms.add(algorithm[1]);
    }
  }
  const coded = new Set();
  for (const source of members.filter((member) => member.startsWith("crates/")).flatMap((member) => rustFiles(`${member}/src`))) {
    for (const [, algorithm] of read(source).matchAll(/\balgorithm: "([a-z0-9_]+)"\.to_owned\(\)/g)) coded.add(algorithm);
  }
  if (coded.size === 0) problem("no `algorithm: \"...\".to_owned()` found in the solver crates; update scripts/check-docs.mjs");
  for (const algorithm of coded) {
    if (!stageAlgorithms.has(algorithm)) problem(`docs/model.md: the stage table does not list the algorithm \`${algorithm}\` that the solvers report`);
  }
  const methods = new Set(rows.map(([method]) => /^`([a-z0-9_]+)`$/.exec(method)?.[1]).filter(Boolean));
  for (const algorithm of stageAlgorithms) {
    if (!methods.has(algorithm)) problem(`${file}: the methods table has no row whose first column is \`${algorithm}\` (stage table of docs/model.md)`);
  }
}

// ---- Refinement studies name their command (SPEC-VER-002, SPEC-EVD-001) ----------------
// A section of a physics page whose heading names a refinement or order study contains a
// `cargo test` command that runs an existing test file, and every test-name filter of the
// command matches a test function of that file.
for (const page of pages.filter((page) => page.startsWith("docs/physics/"))) {
  const lines = read(page).split("\n");
  let fence = false;
  const headings = [];
  lines.forEach((line, index) => {
    if (/^```/.test(line)) fence = !fence;
    const heading = !fence && /^(#{2,3}) +(.+)$/.exec(line);
    if (heading) headings.push({ level: heading[1].length, title: heading[2], index });
  });
  headings.forEach((heading, position) => {
    if (!/\brefinement\b|\border stud(?:y|ies)\b/i.test(heading.title)) return;
    const end = headings.slice(position + 1).find((next) => next.level <= heading.level)?.index ?? lines.length;
    // Join shell continuations, and inline code that wraps onto the next line.
    let openCode = false;
    const body = lines
      .slice(heading.index + 1, end)
      .reduce((joined, line) => {
        if (/^```/.test(line)) {
          openCode = false;
          return `${joined}\n${line}`;
        }
        const next = openCode ? `${joined} ${line}` : `${joined}\n${line}`;
        if ((line.match(/`/g) ?? []).length % 2 === 1) openCode = !openCode;
        return next;
      }, "")
      .replace(/\\\n\s*/g, " ");
    const commands = [...body.matchAll(/cargo test ([^`|;)\n]*)/g)].map((match) => match[1]);
    if (commands.length === 0) {
      problem(`${page}: the section "${heading.title}" names no \`cargo test\` command that reproduces it`);
      return;
    }
    for (const command of commands) {
      const tokens = command.trim().split(/\s+/);
      let target;
      const filters = [];
      for (let i = 0; i < tokens.length; i += 1) {
        const token = tokens[i];
        if (["-p", "--package", "--target", "--features", "--manifest-path", "-j"].includes(token)) i += 1;
        else if (token === "--test") target = tokens[(i += 1)];
        else if (!token.startsWith("-") && /^[a-z_][a-z0-9_]*$/.test(token)) filters.push(token);
      }
      if (!target) {
        problem(`${page}: the command "cargo test ${command.trim()}" in "${heading.title}" names no --test file`);
        continue;
      }
      const testFile = `crates/caekit/tests/${target}.rs`;
      if (!requireFile(testFile, `named by a refinement command in ${page}`)) continue;
      const functions = [...read(testFile).matchAll(/\bfn ([a-z_][a-z0-9_]*)/g)].map((match) => match[1]);
      for (const filter of filters) {
        if (!functions.some((name) => name.includes(filter))) problem(`${page}: the filter ${filter} of "cargo test ${command.trim()}" matches no test in ${testFile}`);
      }
    }
  });
}

// ---- Files that every change relies on -------------------------------------------------
if (requireFile("CHANGELOG.md", "release notes come from it") && !/^## \[Unreleased\]$/m.test(read("CHANGELOG.md"))) {
  problem("CHANGELOG.md has no '## [Unreleased]' section");
}
if (requireFile("CLAUDE.md", "it points Claude Code at AGENTS.md") && read("CLAUDE.md").trim() !== "@AGENTS.md") {
  problem("CLAUDE.md must contain only '@AGENTS.md': the rules live in one file");
}
for (const file of ["AGENTS.md", "CONTRIBUTING.md", "SECURITY.md", "docs/ARCHITECTURE.md", "docs/RELEASING.md", "docs/model.md", "docs/provenance.md", "docs/python.md", "docs/wasm.md"]) {
  requireFile(file, "AGENTS.md refers to it");
}

if (problems.length > 0) {
  console.error(`check-docs: ${problems.length} problem${problems.length === 1 ? "" : "s"}\n`);
  for (const message of problems) console.error(`  - ${message}`);
  process.exit(1);
}
console.log(`check-docs: ${pages.length} pages, ${kinds.length} physics kinds and ${members.length} workspace members are consistent`);
