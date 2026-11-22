// The Node build of the package solves the saved reference models.
//   node crates/caekit-wasm/build.mjs && node --test "crates/caekit-wasm/tests/*_test.mjs"
import assert from "node:assert/strict";
import test from "node:test";

import { check, solve, version } from "../npm/node/caekit_wasm.mjs";
import { example, examples } from "./helpers.mjs";

// The limits of the checks below are those of scripts/verify-reference-cases.mjs, where each is
// derived and measured; the package runs the same solver code, so the same margins apply.
function close(actual, expected, tolerance, label) {
  assert.ok(Math.abs(actual - expected) <= tolerance, `${label}: expected ${expected} ± ${tolerance}, measured ${actual}`);
}

test("every reference model is present", () => {
  assert.ok(examples.length >= 7, `found ${examples.length} models`);
});

for (const { name, text, model } of examples) {
  test(`examples/${name}.json converges`, () => {
    const checked = check(text);
    assert.equal(checked.model_schema_version, 1);
    assert.ok(checked.meshes.length > 0);

    const report = solve(text);
    assert.equal(report.schema_version, 1);
    assert.equal(report.model_name, model.name);
    assert.equal(report.stop_reason, "converged");
    assert.equal(report.execution.backend, "wasm_cpu");
    assert.equal(report.execution.solver_version, version());
    assert.deepEqual(Object.keys(report.quantities).sort(), [...model.outputs].sort());
    assert.deepEqual(report.meshes, checked.meshes);
    for (const stage of report.convergence) {
      assert.equal(stage.stop_reason, "converged");
      assert.ok(stage.final_true_residual_l2 <= stage.residual_threshold, stage.stage);
    }
  });
}

test("analytical references", () => {
  const bar = solve(example("axial_bar").text);
  // 2e-9 of the 5e-5 m extension and 8 times the algebraic error the solver threshold allows
  // (1.3e-14 m); the exact P1 solution deviates by 6.8e-21 m.
  close(bar.quantities.end_displacement.values.tip, 5.0e-5, 1e-13, "axial bar extension (m)");

  const heat = solve(example("heat_conduction").text).quantities.boundary_heat_rate.values;
  // 5e-11 of the 200 W boundary heat rate; the affine solution deviates by 1.1e-13 W.
  close(heat.x0, 200, 1e-8, "heat rate x0 (W)");
  close(heat.x1, -200, 1e-8, "heat rate x1 (W)");

  const [transfer, ...rest] = solve(example("multimaterial_heat_conduction").text).quantities.interface_heat_rate.values;
  assert.equal(rest.length, 0);
  assert.deepEqual([transfer.from_region, transfer.to_region], ["left", "right"]);
  // 1e-9 of the 10 W transfer; the piecewise affine solution deviates by 0 W.
  close(transfer.value, 10, 1e-8, "interface transfer (W)");

  const heated = solve(example("thermoelastic_solid").text);
  // 2.8e-9 of the 3.6 J restrained scale; the solution deviates by 8.9e-16 J.
  close(heated.quantities.elastic_energy.value, 3.6, 1e-8, "elastic energy (J)");
});

test("text, bytes and objects give the same report", () => {
  const { text, model } = example("laminar_channel");
  const fromText = solve(text);
  assert.deepEqual(solve(new TextEncoder().encode(text)), fromText);
  assert.deepEqual(solve(model), fromText);
  // The digest covers the typed model, so key order and indentation do not change it.
  const reordered = Object.fromEntries(Object.entries(model).reverse());
  assert.equal(solve(reordered).model_sha256, fromText.model_sha256);
});

test("errors carry a kind", () => {
  const { model } = example("axial_bar");
  const thrown = (run) => {
    try {
      run();
    } catch (error) {
      return error;
    }
    assert.fail("expected an error");
  };

  for (const [run, kind, message] of [
    [() => solve({ ...model, schema_version: 2 }), "invalid_model", /unsupported model schema version 2/],
    [() => check({ ...model, surprise: 1 }), "invalid_model", /unknown field/],
    [() => check({ ...model, outputs: [] }), "invalid_model", /outputs/],
    [() => solve("{"), "invalid_model", /invalid model JSON/],
    [() => solve(42), "bad_argument", /model must be/],
    [() => solve(undefined), "bad_argument", /model must be/],
    [() => check(() => model), "bad_argument", /model must be/],
  ]) {
    const error = thrown(run);
    assert.ok(error instanceof Error);
    assert.equal(error.name, "CaekitError");
    assert.equal(error.kind, kind);
    assert.match(error.message, message);
  }
});

test("a run that does not converge is returned, not thrown", () => {
  const { model } = example("elastic_solid");
  const report = solve({
    ...model,
    // The default tolerances spelled out (the object needs all three keys); one iteration cannot
    // reach the threshold, since the elastic_solid reference needs 12.
    linear_solver: { relative_residual_tolerance: 1e-10, absolute_residual_tolerance: 0, max_iterations: 1 },
  });
  assert.equal(report.stop_reason, "iteration_limit");
  assert.equal(report.convergence[0].iterations, 1);
  assert.deepEqual(report.quantities, {});
  assert.deepEqual(report.balances, []);
});
