import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const [nativePath, wasiPath] = process.argv.slice(2);
if (!nativePath || !wasiPath || process.argv.length !== 4) {
  console.error("Usage: node scripts/verify-reference-cases.mjs <caekit> <caekit.wasm>");
  process.exit(2);
}

const root = fileURLToPath(new URL("../", import.meta.url));
const runner = fileURLToPath(new URL("./run-wasi.mjs", import.meta.url));

// Relative agreement between native and WASI results. AGENTS.md section 2 fixes it at 1e-10 for
// every reference model. Reason: it equals the default relative residual tolerance of the linear
// solver (LinearSettings::default), the accuracy to which the solved system is defined, and the
// stage iteration counts are compared exactly, so a difference at this size can only come from
// rounding. Measured on the seven reference models: the difference is exactly zero, because the
// report digests of both targets are equal (run this script and read the SHA-256 lines).
const RELATIVE_AGREEMENT = 1e-10;

// Absolute floors of the comparison between targets, for values that are zero or rounding noise
// around zero, where a relative bound cannot hold. The floor of a unit is a multiple k of the
// rounding unit epsilon (2.2e-16) times the scale S of that unit, the largest absolute value of
// any quantity, balance term or imbalance of that unit in the seven reference reports (measured
// with `caekit run examples/<kind>.json` for every kind). Noise around zero comes from sums of
// terms of that scale, so S is the right size to measure it against; k is the margin over a plain
// sum of such terms. The floors act only on near-zero values: elsewhere RELATIVE_AGREEMENT is the
// larger bound. Every floor below states S and k. No single rule produced them: k ranges from 3.8
// to 4.5e6, and the measured native-WASI difference is zero for every unit, so each floor is
// margin for other targets.
const absoluteTolerance = new Map([
  // S = 1.0e-4 m (displacement of elastic_solid); k = 4.5e6. The displacement of
  // thermoelastic_solid deviates from its analytical values by at most 3.4e-21 m (measured).
  ["m", 1e-13],
  // S = 1.2e8 Pa (cauchy_stress of thermoelastic_solid); k = 3.8. The measured error of that
  // stress against the analytical -120 MPa is 8.9e-8 Pa, 3.3 epsilon of S (rounding of its
  // largest term), so this floor is only 1.1 times the measured rounding error.
  ["Pa", 1e-7],
  // S = 2.0e6 N (solid_force_x applied, elastic_solid); k = 23. Measured noise around zero:
  // 1.6e-10 N (thermoelastic_solid solid_force_z imbalance).
  ["N", 1e-8],
  // S = 5.0e5 N m (solid_moment_z reaction, elastic_solid); k = 90. Measured noise around zero:
  // 2.2e-10 N m (thermoelastic_solid solid_moment_y imbalance).
  ["N m", 1e-8],
  // S = 1.5e-2 m/s (axial_velocity of laminar_channel); k = 3.0e5.
  ["m/s", 1e-12],
  // S = 1.0e-5 m^3/s (volume_flow_rate of laminar_channel); k = 450.
  ["m^3/s", 1e-18],
  // S = 1.0e-2 kg/s (mass_flow_rate of cooling_channel); k = 450.
  ["kg/s", 1e-15],
  // S = 2.0e2 (channel_reynolds of laminar_channel); k = 225.
  ["1", 1e-11],
  // S = 4.0e2 K (solid_temperature of heat_conduction); k = 1.1e4.
  ["K", 1e-9],
  // S = 2.1e3 W (wall_energy hot_face_input of cooling_channel); k = 2.1e4. Measured noise
  // around zero: 1.6e-13 W (heat_conduction solid_energy imbalance).
  ["W", 1e-8],
  // S = 2.1e4 W/m^2 (interface_heat_flux of cooling_channel); k = 2.1e4. Measured noise around
  // zero: 1.1e-11 W/m^2 (heat_conduction solid_heat_flux, x component against 20000).
  ["W/m^2", 1e-7],
  // S = 1.0e2 J (elastic_energy of elastic_solid); k = 4.5e5.
  ["J", 1e-8],
]);

function close(actual, expected, tolerance, label) {
  assert.ok(
    Number.isFinite(actual) && Number.isFinite(expected) &&
      Math.abs(actual - expected) <= tolerance,
    `${label}: expected ${expected} ± ${tolerance}, measured ${actual}`,
  );
}

function compareNumbers(actual, expected, floor, relative, label) {
  if (Array.isArray(expected)) {
    assert.ok(Array.isArray(actual), `${label}: expected an array, measured ${actual}`);
    assert.equal(actual.length, expected.length, `${label}: array length`);
    for (const [index, value] of expected.entries()) {
      compareNumbers(actual[index], value, floor, relative, `${label}[${index}]`);
    }
  } else {
    close(actual, expected, Math.max(floor, relative * Math.abs(expected)), label);
  }
}

function compareNamedNumbers(actual, expected, floor, label) {
  assert.deepEqual(Object.keys(actual).sort(), Object.keys(expected).sort(), `${label}: keys`);
  for (const [name, value] of Object.entries(expected)) {
    compareNumbers(actual[name], value, floor, RELATIVE_AGREEMENT, `${label}.${name}`);
  }
}

function comparePhysics(wasi, native, model, label) {
  assert.equal(wasi.model_sha256, native.model_sha256, `${label}: model_sha256`);
  assert.equal(wasi.method, native.method, `${label}: solver method`);
  assert.deepEqual(wasi.geometry, native.geometry, `${label}: geometry revisions`);
  assert.equal(wasi.stop_reason, native.stop_reason, `${label}: run stop reason`);
  const decisions = (report) => report.convergence.map(({ stage, iterations, stop_reason }) => ({
    stage, iterations, stop_reason,
  }));
  assert.deepEqual(decisions(wasi), decisions(native), `${label}: stage decisions`);
  for (const key of model.outputs) {
    const actual = wasi.quantities[key];
    const expected = native.quantities[key];
    const quantity = `${label}: ${key}`;
    assert.equal(actual.kind, expected.kind, `${quantity}: kind`);
    assert.equal(actual.unit, expected.unit, `${quantity}: unit`);
    const floor = absoluteTolerance.get(expected.unit);
    assert.ok(floor, `${quantity}: missing tolerance for unit ${expected.unit}`);
    const metric = `${quantity} (${expected.unit})`;
    switch (expected.kind) {
      case "scalar":
        compareNumbers(actual.value, expected.value, floor, RELATIVE_AGREEMENT, metric);
        break;
      case "by_region":
        compareNamedNumbers(actual.values, expected.values, floor, metric);
        break;
      case "by_interface": {
        const directions = (quantity) => quantity.values.map(({ from_region, to_region }) => [from_region, to_region]);
        assert.deepEqual(directions(actual), directions(expected), `${quantity}: oriented region pairs`);
        compareNumbers(
          actual.values.map(({ value }) => value),
          expected.values.map(({ value }) => value),
          floor, RELATIVE_AGREEMENT, metric,
        );
        break;
      }
      case "field":
      case "vector_field":
      case "tensor_field":
        assert.equal(actual.location, expected.location, `${quantity}: sample location`);
        for (const field of [actual, expected]) {
          assert.ok(field.positions_m.length > 0, `${quantity}: no sample positions`);
          assert.equal(field.values.length, field.positions_m.length, `${quantity}: sample count`);
        }
        // Sample positions are model vertices or centroids (a quarter of a sum of four vertex
        // coordinates). They use basic arithmetic only, so a target can differ only by the
        // rounding of that sum (for example fused operations): at most 4 * epsilon/2 * 3 m =
        // 1.3e-15 m for the largest coordinate (3 m, elastic_solid). 1e-12 m is about 750 times
        // that, and a relative part is meaningless for coordinates at zero.
        compareNumbers(actual.positions_m, expected.positions_m, 1e-12, 0, `${quantity}: positions (m)`);
        compareNumbers(actual.values, expected.values, floor, RELATIVE_AGREEMENT, metric);
        break;
      default:
        assert.fail(`${quantity}: unsupported kind ${expected.kind}`);
    }
  }

  const balanceNames = (report) => report.balances.map(({ name }) => name).sort();
  assert.deepEqual(balanceNames(wasi), balanceNames(native), `${label}: balance names`);
  for (const expected of native.balances) {
    const actual = wasi.balances.find(({ name }) => name === expected.name);
    const balance = `${label}: ${expected.name}`;
    assert.equal(actual.unit, expected.unit, `${balance}: unit`);
    const floor = absoluteTolerance.get(expected.unit);
    assert.ok(floor, `${balance}: missing tolerance for unit ${expected.unit}`);
    const metric = `${balance} (${expected.unit})`;
    compareNamedNumbers(actual.terms, expected.terms, floor, metric);
    compareNumbers(actual.imbalance, expected.imbalance, floor, RELATIVE_AGREEMENT, `${metric}: imbalance`);
  }
}

function run(executable, args, label) {
  const result = spawnSync(executable, args, { cwd: root, encoding: "utf8" });
  if (result.error) {
    throw new Error(`${label}: ${result.error.message}`, { cause: result.error });
  }
  assert.equal(
    result.status,
    0,
    `${label}: expected exit 0, measured ${result.signal ?? result.status}\n${result.stderr.trim()}`,
  );
  try {
    const report = JSON.parse(result.stdout);
    // Preserve the serialized floats, including the sign of zero. The three
    // target-specific execution fields are the only excluded bytes.
    const comparable = result.stdout.replace(
      /^([ \t]*"(?:backend|architecture|operating_system)": )"[^"\r\n]*"/gm,
      '$1""',
    );
    const digest = createHash("sha256").update(comparable.replace(/\r\n/g, "\n")).digest("hex");
    return { report, digest };
  } catch (error) {
    throw new Error(`${label}: invalid JSON report: ${error.message}`, { cause: error });
  }
}

function verifyReport(report, model, backend, stageCount, label) {
  assert.equal(report.schema_version, 1, `${label}: report schema version`);
  assert.equal(report.model_schema_version, model.schema_version, `${label}: model schema version`);
  assert.equal(report.model_name, model.name, `${label}: model name`);
  assert.match(report.model_sha256, /^[0-9a-f]{64}$/, `${label}: model SHA-256`);
  assert.equal(report.execution.backend, backend, `${label}: execution backend`);
  assert.equal(report.execution.precision, "ieee754_binary64", `${label}: execution precision`);
  assert.equal(report.stop_reason, "converged", `${label}: run stop reason`);
  assert.equal(report.convergence.length, stageCount, `${label}: number of solver stages`);
  for (const stage of report.convergence) {
    const { final_true_residual_l2: residual, residual_threshold: threshold } = stage;
    const description = `${label}: ${stage.stage}`;
    assert.equal(stage.stop_reason, "converged", `${description}: stop reason`);
    assert.ok(
      Number.isFinite(threshold) && Number.isFinite(residual) && residual >= 0 && residual <= threshold,
      `${description}: expected final residual ≤ ${threshold} ${stage.residual_unit}, measured ${residual}`,
    );
  }
  assert.deepEqual(Object.keys(report.quantities).sort(), [...model.outputs].sort(), `${label}: requested outputs`);
}

function referenceQuantity(report, name, unit, label) {
  const quantity = report.quantities[name];
  assert.equal(quantity?.unit, unit, `${label}: ${name} unit`);
  return quantity;
}

function referenceField(report, name, unit, kind, location, label) {
  const field = referenceQuantity(report, name, unit, label);
  assert.equal(field.kind, kind, `${label}: ${name} kind`);
  assert.equal(field.location, location, `${label}: ${name} sample location`);
  return field;
}

function referenceBalance(report, name, unit, label) {
  const balance = report.balances.find((entry) => entry.name === name);
  assert.equal(balance?.unit, unit, `${label}: ${name} balance unit`);
  return balance;
}

// Use the analytical references and acceptance limits in crates/caekit/tests/<case>.rs.
//
// The comment of each limit below states where it comes from. "Measured" values are the deviation
// of the report of `caekit run examples/<kind>.json` (native build of this change) from the
// analytical value of the same check. The affine and uniform references are exact in the linear
// element space, so their error is rounding plus the algebraic error of the solve; the solves of
// the references end far below their residual threshold, so the measured deviations are at
// rounding level. A relative figure is the limit divided by the scale of the quantity.
const references = {
  axial_bar(report, label) {
    const tip = referenceQuantity(report, "end_displacement", "m", label).values?.tip;
    // The P1 bar reproduces the tip extension exactly (docs/physics/axial-bar.md), so the error is
    // algebraic: at most threshold / lambda_min(K) = 1e-7 N / 7.98e6 N/m = 1.3e-14 m, with
    // lambda_min of the five-element stiffness matrix at EA = 2e7 N (computed from the example
    // mesh). The run ends at a residual of 2e-12 N and measures 6.8e-21 m. 1e-13 m is 8 times the
    // threshold bound and 2e-9 of the 5e-5 m extension.
    close(tip, 5e-5, 1e-13, `${label}: end_displacement.tip (m)`);
    return `tip extension ${tip} m`;
  },
  elastic_solid(report, label) {
    const energy = referenceQuantity(report, "elastic_energy", "J", label).value;
    const reaction = referenceBalance(report, "solid_force_x", "N", label).terms.reaction;
    // 1e-6 J is 1e-8 of the 100 J energy: 100 times the default relative residual tolerance, since
    // an algebraic displacement error changes the energy linearly. Measured 2.7e-13 J.
    close(energy, 100, 1e-6, `${label}: elastic_energy (J)`);
    // The recovered reaction differs from the applied load by the sum of the solver residual over
    // the free x degrees of freedom (the stiffness has the translation in x in its null space, so
    // the internal forces sum to zero): at most sqrt(4) * 1.054e-4 N = 2.1e-4 N for the 4 free
    // vertices at the residual threshold. 1e-3 N is 4.7 times that and 5e-10 of the 2e6 N
    // reaction. Measured 2.6e-9 N.
    close(reaction, -2e6, 1e-3, `${label}: solid_force_x.reaction (N)`);
    return `strain energy ${energy} J, x reaction ${reaction} N`;
  },
  laminar_channel(report, label) {
    const drop = referenceQuantity(report, "pressure_loss", "Pa", label).value;
    // Discretization estimate: the 32-cell mesh gives the pressure loss 2.4 Pa / (1 + 2/32^2), an
    // error of 2 * 2.4 / (32^2 + 2) = 4.678e-3 Pa (derived in docs/physics/laminar-channel.md and
    // matched by the measurement to 7e-13 relative). 0.005 Pa is 1.07 times that, rounded up to
    // the next 0.5 mPa, so a coarser or wrongly weighted discretization fails.
    close(drop, 2.4, 0.005, `${label}: pressure_loss (Pa)`);
    return `pressure loss ${drop} Pa`;
  },
  cooling_channel(report, label) {
    const outlet = referenceQuantity(report, "outlet_temperature", "K", label).value;
    const heat = referenceQuantity(report, "heat_transfer_rate", "W", label).value;
    const resistance = 0.004 / 15 + 1 / 400;
    const ntu = (2 * 0.1 / resistance) / (0.01 * 4180);
    // Discretization estimate: first-order upwind on 48 coolant panels (docs/physics/cooling-channel.md)
    // leaves the outlet 0.3405 K from the analytical 349.0018 K (measured); 0.5 K is 1.5 times that
    // and 0.8 percent of the 62 K inlet-to-wall difference.
    close(outlet, 360 - (360 - 298) * Math.exp(-ntu), 0.5, `${label}: outlet_temperature (K)`);
    // The heat rate and the coolant enthalpy rise are the same quantity up to the coolant-cell
    // residual, which the final fluid sweep leaves at rounding level (measured 9.1e-13 W, 2 epsilon
    // of the 2118 W duty). 1e-6 W matches the energy-balance limit below.
    close(heat, 0.01 * 4180 * (outlet - 298), 1e-6, `${label}: heat_transfer_rate (W)`);
    for (const name of ["wall_energy", "interface_energy", "coolant_energy", "system_energy"]) {
      const { imbalance } = referenceBalance(report, name, "W", label);
      // docs/physics/cooling-channel.md states 1e-6 W. The wall balances close only to the coupling
      // iteration: measured 6.3e-8 W against a final coupling residual of 2.2e-8 W and a threshold
      // of 8.35e-7 W (the sum of 31 wall residuals is at most sqrt(31) times their norm, so
      // 4.7e-6 W at the threshold, above this limit: the limit rests on the measured margin of 16).
      // The coolant and interface balances measured 4.5e-13 W.
      close(imbalance, 0, 1e-6, `${label}: ${name}.imbalance (W)`);
    }
    return `outlet temperature ${outlet} K, heat transfer ${heat} W`;
  },
  heat_conduction(report, label) {
    assert.equal(report.convergence[0].stage, "solid_heat_conduction");
    assert.equal(report.convergence[0].residual_unit, "W");
    const temperature = referenceField(report, "solid_temperature", "K", "field", "node", label);
    for (const [index, [x]] of temperature.positions_m.entries()) {
      // Affine field, exact in P1 (docs/physics/heat-conduction.md states 1e-8 K): 2.5e-11 of the
      // 400 K scale. Measured 0 K: the solve takes 1 iteration and ends at a residual of 0 W.
      close(temperature.values[index], 400 - 1000 * x, 1e-8, `${label}: temperature[${index}] (K)`);
    }
    const midpoint = temperature.positions_m.findIndex((point) => point.every((x) => x === 0.05));
    const centerTemperature = temperature.values[midpoint];
    // Same limit and reason as the nodal temperatures above; measured 0 K.
    close(centerTemperature, 350, 1e-8, `${label}: center temperature (K)`);
    const flux = referenceField(report, "solid_heat_flux", "W/m^2", "vector_field", "cell", label);
    for (const [index, value] of flux.values.entries()) {
      // 5e-11 of the 2e4 W/m^2 flux (docs/physics/heat-conduction.md states 1e-6 W/m^2). The
      // flux is k times a sum of nodal differences times gradients; measured 1.1e-11 W/m^2.
      compareNumbers(value, [20_000, 0, 0], 1e-6, 0, `${label}: flux[${index}] (W/m^2)`);
    }
    const heat = referenceQuantity(report, "boundary_heat_rate", "W", label).values;
    const expectedHeat = { x0: 200, x1: -200, y0: 0, y1: 0, z0: 0, z1: 0 };
    assert.deepEqual(Object.keys(heat).sort(), Object.keys(expectedHeat).sort());
    for (const [region, expected] of Object.entries(expectedHeat)) {
      // 5e-11 of the 200 W boundary heat rate (docs/physics/heat-conduction.md states 1e-8 W);
      // measured 1.1e-13 W.
      close(heat[region], expected, 1e-8, `${label}: boundary heat rate ${region} (W)`);
    }
    const { imbalance } = referenceBalance(report, "solid_energy", "W", label);
    // Same 1e-8 W as the heat rates it sums; measured 1.6e-13 W.
    close(imbalance, 0, 1e-8, `${label}: solid energy balance (W)`);
    return `center ${centerTemperature} K, x flux ${flux.values[0][0]} W/m^2, boundary heat rates ${heat.x0} and ${heat.x1} W`;
  },
  multimaterial_heat_conduction(report, label) {
    assert.deepEqual(
      report.convergence.map(({ stage, residual_unit }) => [stage, residual_unit]),
      [["solid_heat_conduction", "W"], ["solid_interface_equilibration", "W"]],
    );
    const temperature = referenceField(report, "solid_temperature", "K", "field", "node", label);
    let interfaceNodes = 0;
    for (const [index, [x]] of temperature.positions_m.entries()) {
      const expected = x <= 0.01 ? 311 - 100 * x : 310 - 1000 * (x - 0.01);
      // Piecewise affine field, exact in P1: 3e-11 of the 311 K scale. Measured 0 K.
      close(temperature.values[index], expected, 1e-8, `${label}: temperature[${index}] (K)`);
      if (x === 0.01) interfaceNodes += 1;
    }
    assert.equal(interfaceNodes, 4, `${label}: shared interface nodes`);
    const flux = referenceField(report, "solid_heat_flux", "W/m^2", "vector_field", "cell", label);
    for (const [index, value] of flux.values.entries()) {
      // 1e-9 of the 1e3 W/m^2 flux; measured 4.5e-13 W/m^2.
      compareNumbers(value, [1_000, 0, 0], 1e-6, 0, `${label}: flux[${index}] (W/m^2)`);
    }
    const boundary = referenceQuantity(report, "boundary_heat_rate", "W", label).values;
    // 1e-9 of the 10 W boundary heat rate, and the 1e-8 W balance limit of
    // docs/physics/multimaterial-heat-conduction.md; measured 0 W.
    compareNamedNumbers(boundary, { x0: 10, x1: -10, y0: 0, y1: 0, z0: 0, z1: 0 }, 1e-8, label);
    const contact = referenceQuantity(report, "interface_heat_rate", "W", label);
    assert.equal(contact.kind, "by_interface");
    assert.equal(contact.values.length, 1);
    const [transfer] = contact.values;
    assert.deepEqual([transfer.from_region, transfer.to_region], ["left", "right"]);
    // Same 1e-8 W limit as the boundary heat rates; measured 0 W.
    close(transfer.value, 10, 1e-8, `${label}: interface transfer (W)`);
    const { imbalance } = referenceBalance(report, "solid_energy", "W", label);
    // Same limit; measured 4.4e-16 W.
    close(imbalance, 0, 1e-8, `${label}: solid energy balance (W)`);
    for (const [name, exterior] of [["left", 10], ["right", -10]]) {
      const balance = referenceBalance(report, `solid_energy:${name}`, "W", label);
      compareNamedNumbers(
        balance.terms,
        { exterior_heat_input: exterior, interface_heat_input: -exterior, volumetric_generation: 0 },
        // Same 1e-8 W limit; measured 0 W for the terms and the regional imbalances.
        1e-8, `${label}: ${name} energy terms (W)`,
      );
      // Regional imbalance: the same 1e-8 W limit; measured 0 W.
      close(balance.imbalance, 0, 1e-8, `${label}: ${name} energy balance (W)`);
    }
    return `interface 310 K, x flux ${flux.values[0][0]} W/m^2, transfer ${transfer.value} W left to right`;
  },
  thermoelastic_solid(report, label) {
    assert.deepEqual(
      report.convergence.map(({ stage, residual_unit }) => [stage, residual_unit]),
      [["solid_heat_conduction", "W"], ["solid_elastic_equilibrium", "N"]],
    );
    const temperature = referenceField(report, "solid_temperature", "K", "field", "node", label);
    for (const [index, value] of temperature.values.entries()) {
      // Uniform field (only x0 is held, the other faces are insulated): 2.9e-11 of the 350 K scale
      // (docs/physics/thermoelastic-solid.md uses the same 1e-8 K family). Measured 0 K.
      close(value, 350, 1e-8, `${label}: temperature[${index}] (K)`);
    }
    const flux = referenceField(report, "solid_heat_flux", "W/m^2", "vector_field", "cell", label);
    for (const [index, value] of flux.values.entries()) {
      // The flux of a uniform field is zero; 1e-6 W/m^2 is the limit of the heat_conduction case
      // (5e-11 of its 2e4 W/m^2 scale). Measured 0 W/m^2.
      compareNumbers(value, [0, 0, 0], 1e-6, 0, `${label}: flux[${index}] (W/m^2)`);
    }
    const boundary = referenceQuantity(report, "boundary_heat_rate", "W", label);
    for (const [region, power] of Object.entries(boundary.values)) {
      // Zero heat rate through every region, same 1e-8 W as the heat_conduction case; measured 0 W.
      close(power, 0, 1e-8, `${label}: boundary heat rate ${region} (W)`);
    }
    const displacement = referenceField(report, "displacement_vector", "m", "vector_field", "node", label);
    for (const [index, [, y, z]] of displacement.positions_m.entries()) {
      // docs/physics/thermoelastic-solid.md: the absolute allowances cover solver and rounding
      // error on the field scales. The largest transverse displacement is 0.00078 * 0.01 m =
      // 7.8e-6 m, so 5e-12 m is 6.4e-7 of it; zero components are held to the same limit.
      // Measured at most 3.4e-21 m.
      compareNumbers(displacement.values[index], [0, 0.00078 * y, 0.00078 * z], 5e-12, 0,
        `${label}: displacement[${index}] (m)`);
    }
    const stress = referenceField(report, "cauchy_stress", "Pa", "tensor_field", "cell", label);
    for (const [index, value] of stress.values.entries()) {
      // 0.05 Pa is 4.2e-10 of the 120 MPa stress (docs/physics/thermoelastic-solid.md). Measured
      // 8.9e-8 Pa, 3.3 epsilon of the stress scale (the stress is E times a strain sum), so the
      // margin is 5.6e5.
      compareNumbers(value, [[-120e6, 0, 0], [0, 0, 0], [0, 0, 0]], 0.05, 0,
        `${label}: stress[${index}] (Pa)`);
    }
    const vonMises = referenceField(report, "von_mises_stress", "Pa", "field", "cell", label);
    for (const [index, value] of vonMises.values.entries()) {
      // Same 0.05 Pa limit as the stress it is computed from; measured 4.5e-8 Pa.
      close(value, 120e6, 0.05, `${label}: von Mises stress[${index}] (Pa)`);
    }
    const reaction = referenceField(report, "nodal_reaction", "N", "vector_field", "node", label);
    const endForce = [0, 0];
    for (const [index, [x]] of reaction.positions_m.entries()) {
      assert.ok(x === 0 || x === 1, `${label}: reaction plane ${x} m`);
      endForce[x] += reaction.values[index][0];
    }
    // 5e-6 N is 4.2e-10 of the 12 kN end reaction (docs/physics/thermoelastic-solid.md states
    // 5e-6 N for support forces). Measured 0 N.
    compareNumbers(endForce, [12_000, -12_000], 5e-6, 0, `${label}: end reactions (N)`);
    const energy = referenceQuantity(report, "elastic_energy", "J", label).value;
    // 1e-8 J is 2.8e-9 of the 3.6 J restrained scale (docs/physics/thermoelastic-solid.md);
    // measured 8.9e-16 J.
    close(energy, 3.6, 1e-8, `${label}: elastic energy (J)`);
    for (const [name, unit, tolerance] of [
      // Energy balance: the heat-conduction limit of 1e-8 W; measured 0 W.
      ["solid_energy", "W", 1e-8],
      // Force balance: the 5e-6 N support-force limit above, 4.2e-10 of the 12 kN reaction;
      // measured at most 1.6e-10 N (the z component).
      ...["x", "y", "z"].map((axis) => [`solid_force_${axis}`, "N", 5e-6]),
      // Moment balance: the same numerical limit in N m, which a lever arm of 1 m (the bar length)
      // turns from the 5e-6 N force limit; measured at most 2.2e-10 N m.
      ...["x", "y", "z"].map((axis) => [`solid_moment_${axis}`, "N m", 5e-6]),
    ]) {
      const { imbalance } = referenceBalance(report, name, unit, label);
      close(imbalance, 0, tolerance, `${label}: ${name} balance (${unit})`);
    }
    return `axial stress ${stress.values[0][0][0]} Pa, end reactions ${endForce.join(" and ")} N, elastic energy ${energy} J`;
  },
};

for (const [name, reference] of Object.entries(references)) {
  const fixture = `examples/${name}.json`;
  test(fixture, (t) => {
    const model = JSON.parse(readFileSync(resolve(root, fixture), "utf8"));
    const stages = ["cooling_channel", "multimaterial_heat_conduction", "thermoelastic_solid"].includes(name) ? 2 : 1;
    const { report: native, digest: nativeDigest } =
      run(resolve(nativePath), ["run", fixture], `${fixture} (native)`);
    const { report: wasi, digest: wasiDigest } =
      run(process.execPath, [runner, resolve(wasiPath), "run", fixture], `${fixture} (WASI)`);
    for (const [report, backend, target] of [
      [native, "native_cpu", "native"],
      [wasi, "wasm_cpu", "WASI"],
    ]) {
      const label = `${fixture} (${target})`;
      verifyReport(report, model, backend, stages, label);
      t.diagnostic(`${target}: ${reference(report, label)}`);
    }
    comparePhysics(wasi, native, model, `${fixture} (WASI vs native)`);
    const histories = (report) => report.convergence.map(({ residual_l2_history }) => residual_l2_history);
    const equalHistories = histories(native).every((history, index) => {
      const other = histories(wasi)[index];
      return history.length === other.length && history.every((value, step) => Object.is(value, other[step]));
    });
    t.diagnostic(`Exact residual histories: ${equalHistories}`);
    t.diagnostic(`Report SHA-256 (normalized platform fields): native ${nativeDigest}, WASI ${wasiDigest}`);
  });
}
