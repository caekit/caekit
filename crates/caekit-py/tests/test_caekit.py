"""Tests of the installed `caekit` wheel against the saved reference models."""

import json
import pathlib
import re
import sys
import threading

import pytest

import caekit

REPO = pathlib.Path(__file__).resolve().parents[3]
EXAMPLES = sorted((REPO / "examples").glob("*.json"))


def example(name: str) -> pathlib.Path:
    return REPO / "examples" / f"{name}.json"


def test_examples_are_present():
    assert len(EXAMPLES) >= 7


@pytest.mark.parametrize("path", EXAMPLES, ids=lambda path: path.stem)
def test_reference_model_converges(path):
    model = json.loads(path.read_text(encoding="utf-8"))
    check = caekit.check(path)
    assert check["model_schema_version"] == caekit.MODEL_SCHEMA_VERSION
    assert check["meshes"]

    report = caekit.solve(path)
    assert set(report) == {
        "schema_version", "model_schema_version", "model_name", "model_sha256",
        "geometry", "meshes", "execution", "method", "stop_reason", "convergence",
        "balances", "quantities",
    }
    assert set(report["execution"]) == {
        "backend", "precision", "architecture", "operating_system", "threads", "solver_version",
    }
    assert report["schema_version"] == caekit.RESULT_SCHEMA_VERSION
    assert report["model_name"] == model["name"]
    assert report["stop_reason"] == "converged"
    assert report["execution"]["backend"] == "native_cpu"
    assert report["execution"]["solver_version"] == caekit.__version__
    assert set(report["quantities"]) == set(model["outputs"])
    assert report["meshes"] == check["meshes"]
    for stage in report["convergence"]:
        assert stage["stop_reason"] == "converged"
        assert stage["final_true_residual_l2"] <= stage["residual_threshold"]


def test_analytical_references():
    # The limits below are those of scripts/verify-reference-cases.mjs, where each is derived and
    # measured; the wheel runs the same solver code, so the same margins apply.
    bar = caekit.solve(example("axial_bar"))
    tip = bar["quantities"]["end_displacement"]["values"]["tip"]
    # 2e-9 of the 5e-5 m extension and 8 times the algebraic error the solver threshold allows
    # (1.3e-14 m); the exact P1 solution deviates by 6.8e-21 m.
    assert tip == pytest.approx(5.0e-5, abs=1e-13)

    heat = caekit.solve(example("heat_conduction"))
    rates = heat["quantities"]["boundary_heat_rate"]["values"]
    # 5e-11 of the 200 W boundary heat rate; the affine solution deviates by 1.1e-13 W.
    assert rates["x0"] == pytest.approx(200.0, abs=1e-8)
    assert rates["x1"] == pytest.approx(-200.0, abs=1e-8)

    layers = caekit.solve(example("multimaterial_heat_conduction"))
    (transfer,) = layers["quantities"]["interface_heat_rate"]["values"]
    assert (transfer["from_region"], transfer["to_region"]) == ("left", "right")
    # 1e-9 of the 10 W transfer; the piecewise affine solution deviates by 0 W.
    assert transfer["value"] == pytest.approx(10.0, abs=1e-8)

    heated = caekit.solve(example("thermoelastic_solid"))
    # 2.8e-9 of the 3.6 J restrained scale; the solution deviates by 8.9e-16 J.
    assert heated["quantities"]["elastic_energy"]["value"] == pytest.approx(3.6, abs=1e-8)


def test_every_input_form_gives_the_same_report():
    path = example("laminar_channel")
    text = path.read_text(encoding="utf-8")
    forms = [path, text, text.encode("utf-8"), bytearray(text.encode("utf-8")), json.loads(text)]
    reports = [caekit.solve(form) for form in forms]
    for report in reports[1:]:
        assert report == reports[0]
    checks = [caekit.check(form) for form in forms]
    for check in checks[1:]:
        assert check == checks[0]
    # The digest covers the typed model, so key order and indentation do not change it.
    reordered = dict(reversed(list(json.loads(text).items())))
    assert caekit.solve(reordered)["model_sha256"] == reports[0]["model_sha256"]


def test_solve_releases_the_interpreter_lock():
    from caekit._caekit import solve_json

    if not getattr(sys, "_is_gil_enabled", lambda: True)():
        pytest.skip("This interpreter is running without a GIL")

    model = json.loads(example("axial_bar").read_text(encoding="utf-8"))
    # Give the waiting thread a scheduling opportunity inside a bounded solve.
    # The test compares event order and does not assert a wall-clock duration.
    cells = 65_536
    model["physics"]["mesh"]["vertices_m"] = [i / cells for i in range(cells + 1)]
    model["outputs"] = ["end_displacement"]
    model["linear_solver"] = {
        # The default value (LinearSettings::default), spelled out because the object needs all
        # three keys; only the iteration limit below is chosen for this test.
        "relative_residual_tolerance": 1e-10,
        "absolute_residual_tolerance": 0.0,
        # Only bounds the run time of a 65,536-cell bar; the test compares event order and does
        # not depend on whether the solve converges within the limit.
        "max_iterations": 16,
    }
    document = json.dumps(model).encode("utf-8")
    entered = threading.Event()
    ready = threading.Event()
    ran = threading.Event()
    ran_before_return = []

    def worker():
        ready.set()
        entered.wait()
        ran.set()

    def observe_native_call(_frame, event, function):
        if function is solve_json:
            if event == "c_call":
                entered.set()
            elif event == "c_return":
                ran_before_return.append(ran.is_set())

    thread = threading.Thread(target=worker)
    thread.start()
    ready.wait()
    original_interval = sys.getswitchinterval()
    original_profile = sys.getprofile()
    try:
        # Suppress ordinary bytecode switching between the profiler's entry
        # event and native entry. The waiting thread needs the native detach.
        sys.setswitchinterval(60.0)
        sys.setprofile(observe_native_call)
        report = caekit.solve(document)
    finally:
        sys.setprofile(original_profile)
        sys.setswitchinterval(original_interval)
        entered.set()
        thread.join()

    assert report["stop_reason"] == "iteration_limit"
    assert ran_before_return == [True]


def test_invalid_models_raise_invalid_model_error():
    model = json.loads(example("axial_bar").read_text(encoding="utf-8"))

    with pytest.raises(caekit.InvalidModelError, match="unsupported model schema version 2"):
        caekit.solve({**model, "schema_version": 2})
    with pytest.raises(caekit.InvalidModelError, match="unknown field"):
        caekit.check({**model, "surprise": 1})
    with pytest.raises(caekit.InvalidModelError, match="outputs"):
        caekit.check({**model, "outputs": []})
    with pytest.raises(caekit.InvalidModelError, match="invalid model JSON"):
        caekit.solve("{")
    with pytest.raises(caekit.InvalidModelError, match="cannot be written as JSON"):
        caekit.solve({**model, "name": float("nan")})
    assert issubclass(caekit.InvalidModelError, caekit.CaekitError)
    assert issubclass(caekit.CalculationError, caekit.CaekitError)


def test_unsupported_argument_types_raise_type_error():
    for value in (None, 1, 1.5, ["schema_version"]):
        with pytest.raises(TypeError, match="model must be"):
            caekit.solve(value)


def test_nonconverged_run_is_reported_not_raised():
    model = json.loads(example("elastic_solid").read_text(encoding="utf-8"))
    model["linear_solver"] = {
        # The default value, spelled out because the object needs all three keys.
        "relative_residual_tolerance": 1e-10,
        "absolute_residual_tolerance": 0.0,
        # One iteration cannot reach the threshold: the elastic_solid reference needs 12.
        "max_iterations": 1,
    }
    report = caekit.solve(model)
    assert report["stop_reason"] == "iteration_limit"
    assert report["convergence"][0]["iterations"] == 1
    assert report["quantities"] == {}
    assert report["balances"] == []


def documentation_examples():
    """Every `python` block of the repository README, of docs/ and of the package README."""
    pages = [REPO / "README.md", REPO / "crates" / "caekit-py" / "README.md"]
    pages += sorted((REPO / "docs").rglob("*.md"))
    examples = []
    for page in pages:
        text = page.read_text(encoding="utf-8")
        for index, match in enumerate(re.finditer(r"^```python\n(.*?)^```", text, re.M | re.S), 1):
            name = f"{page.relative_to(REPO).as_posix()}#{index}"
            examples.append(pytest.param(name, match.group(1), id=name))
    return examples


def test_documentation_has_python_examples():
    names = {example.values[0].split("#")[0] for example in documentation_examples()}
    assert {"README.md", "docs/python.md", "crates/caekit-py/README.md"} <= names


@pytest.mark.parametrize(("name", "source"), documentation_examples())
def test_documentation_example_runs(name, source, monkeypatch, capsys):
    # The examples are written to run from the repository root.
    monkeypatch.chdir(REPO)
    exec(compile(source, name, "exec"), {"__name__": "__main__"})
    assert capsys.readouterr().out, f"{name} prints its result"
