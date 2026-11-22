# caekit

Python bindings for [CAEKit](https://github.com/caekit/caekit), a Rust project for
open-source multiphysics engineering simulation. A model is a versioned JSON document.
`caekit.solve` validates it, runs the reference double-precision CPU solver and returns the
run report, with convergence evidence and conservation balances, as plain dicts and lists.

## Install

```sh
pip install caekit
```

Wheels cover CPython 3.9 and newer on Linux (x86_64, aarch64), macOS (arm64, x86_64) and
Windows (x64). On other platforms pip builds the source distribution, which needs Rust 1.85
or newer.

## Solve a model

```python
import caekit

model = {
    "schema_version": 1,
    "name": "Axial bar under a 1 kN tensile force",
    "units": "si",
    "physics": {
        "kind": "axial_bar",
        "geometry": {
            "identity": {"id": "uniform-bar", "revision": "1"},
            "frame": {"origin_m": [0, 0, 0], "direction": [1, 0, 0]},
            "length_m": 1,
            "cross_section_area_m2": 0.0001,
        },
        "mesh": {"vertices_m": [0, 0.5, 1], "start_region": "clamp", "end_region": "tip"},
        "material": {
            "id": "reference-elastic-solid",
            "young_modulus_pa": 200e9,
            "source": {
                "citation": "Synthetic constant modulus",
                "usage_terms": "Covered by the CAEKit project license",
            },
        },
        "boundary_conditions": [
            {"kind": "displacement", "region": "clamp", "value_m": 0},
            {"kind": "force", "region": "tip", "value_n": 1000},
        ],
    },
    "outputs": ["end_displacement", "axial_stress", "reaction_force"],
}

report = caekit.solve(model)
assert report["stop_reason"] == "converged"

# F L / (E A) = 1000 N * 1 m / (200e9 Pa * 1e-4 m^2) = 5e-5 m
extension = report["quantities"]["end_displacement"]["values"]["tip"]
# Linear elements reproduce the uniform strain exactly, so only the solve and rounding remain;
# 1e-13 m is 2e-9 of the extension, the TIP_DISPLACEMENT_TOL_M of crates/caekit/tests/axial_bar.rs.
assert abs(extension - 5e-5) < 1e-13
print(f"extension {extension:.3e} m, solver {report['execution']['solver_version']}")
```

`solve` and `check` accept the model as a mapping, as JSON text, as UTF-8 JSON bytes or as
a `pathlib.Path` naming a file. Read `report["stop_reason"]` before you use a result: a run
that stops without convergence is returned, not raised. What each outcome returns or raises
is in the
[outcome table](https://github.com/caekit/caekit/blob/master/docs/model.md#run-outcomes).

Both entry points release the interpreter lock during native computation.

## Documentation

- [Python reference](https://github.com/caekit/caekit/blob/master/docs/python.md)
- [Model and result format](https://github.com/caekit/caekit/blob/master/docs/model.md)
- [Physics cases and their analytical references](https://github.com/caekit/caekit/tree/master/docs/physics)
- [Changelog](https://github.com/caekit/caekit/blob/master/CHANGELOG.md)

## License

MIT or Apache-2.0, at your option.
