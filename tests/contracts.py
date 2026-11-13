#!/usr/bin/env python3
"""Check contracts/ (#1020): run as `python3 -I tests/contracts.py` from anywhere.

Every schema must pass Draft 2020-12's own check and carry the rules
contracts/README.md sets. Every examples/<name>/valid-*.json must pass
<name>.schema.json. Every invalid-*.json must fail it with exactly one error,
the keyword and place its errors.json names, so an example cannot fail for a
reason other than the one its name gives. Nothing runs this file automatically
yet (#1021). Uses only the distro's python3-jsonschema; installs nothing.
"""
import json
import pathlib
import sys

try:
    from jsonschema import Draft202012Validator
    from jsonschema.exceptions import SchemaError
except ImportError:
    print("contracts: python3-jsonschema is not installed (apt install python3-jsonschema)")
    sys.exit(2)

ROOT = pathlib.Path(__file__).resolve().parent.parent / "contracts"
DRAFT = "https://json-schema.org/draft/2020-12/schema"


def refuse_constant(word):
    raise ValueError(f"not JSON: {word}")


def unique_keys(pairs):
    keys = [k for k, _ in pairs]
    dup = sorted({k for k in keys if keys.count(k) > 1})
    if dup:
        raise ValueError(f"duplicate keys {dup}: validators keep different ones")
    return dict(pairs)


def load(path):
    """Strict JSON: no NaN or Infinity, no duplicate key."""
    with open(path, encoding="utf-8") as f:
        return json.load(f, object_pairs_hook=unique_keys, parse_constant=refuse_constant)


def house_rules(schema):
    """README's rules a schema states about itself; [] when it keeps them all."""
    bad = []
    if schema.get("$schema") != DRAFT:
        bad.append("$schema is not draft 2020-12")
    if not str(schema.get("$id", "")).startswith("urn:tmux-cockpit:contracts:"):
        bad.append("$id is not urn:tmux-cockpit:contracts:<name>")
    version = schema.get("properties", {}).get("schema_version", {})
    if not isinstance(version.get("const"), int) or isinstance(version.get("const"), bool):
        bad.append("schema_version is not an integer const")
    if "schema_version" not in schema.get("required", []):
        bad.append("schema_version is not required")
    if schema.get("additionalProperties") is not False:
        bad.append("additionalProperties is not false")
    return bad


def check_example(validator, path, want):
    """None when the example behaves, else what went wrong."""
    try:
        doc = load(path)
    except (ValueError, OSError) as e:
        return f"unreadable: {e}"
    errors = list(validator.iter_errors(doc))
    if path.name.startswith("valid-"):
        if not errors:
            return None
        return f"refused: {errors[0].validator} at {errors[0].json_path}: {errors[0].message}"
    if want is None:
        return "no errors.json entry"
    if not errors:
        return "accepted"
    got = [(e.validator, e.json_path) for e in errors]
    if got != [(want.get("keyword"), want.get("at"))]:
        return f"refused for {got}, errors.json wants {want.get('keyword')} at {want.get('at')}"
    return None


def check_dir(name, validator, surprises):
    d = ROOT / "examples" / name
    files = sorted(p for p in d.glob("*.json") if p.name != "errors.json")
    try:
        why = load(d / "errors.json")
    except (ValueError, OSError) as e:
        surprises.append(f"{name}/errors.json: {e}")
        why = {}
    for p in files:
        if not p.name.startswith(("valid-", "invalid-")):
            surprises.append(f"{name}/{p.name}: not valid-* or invalid-*")
            continue
        wrong = check_example(validator, p, why.get(p.name))
        print(f"{'FAIL' if wrong else 'ok  '} {name}/{p.name}{': ' + wrong if wrong else ''}")
        if wrong:
            surprises.append(f"{name}/{p.name}: {wrong}")
    for kind in ("valid-", "invalid-"):
        if not any(p.name.startswith(kind) for p in files):
            surprises.append(f"{name}: no {kind}* example")
    for stale in sorted(set(why) - {p.name for p in files if p.name.startswith("invalid-")}):
        surprises.append(f"{name}/errors.json: names {stale}, which is no invalid-* file here")
    return len(files)


def main():
    surprises = []
    schemas = sorted(ROOT.glob("*.schema.json"))
    names = {p.name.removesuffix(".schema.json") for p in schemas}
    ex = ROOT / "examples"
    dirs = {p.name for p in ex.iterdir() if p.is_dir()} if ex.is_dir() else set()
    for orphan in sorted(dirs - names):
        surprises.append(f"examples/{orphan}: no {orphan}.schema.json")
    count = 0
    for p in schemas:
        name = p.name.removesuffix(".schema.json")
        try:
            schema = load(p)
            Draft202012Validator.check_schema(schema)
        except (ValueError, OSError, SchemaError) as e:
            print(f"FAIL {p.name}: {str(e).splitlines()[0]}")
            surprises.append(f"{p.name}: {str(e).splitlines()[0]}")
            continue
        bad = house_rules(schema)
        print(f"{'FAIL' if bad else 'ok  '} {p.name}{': ' + '; '.join(bad) if bad else ''}")
        surprises += [f"{p.name}: {b}" for b in bad]
        if name not in dirs:
            surprises.append(f"{p.name}: no examples/{name} folder")
            continue
        count += check_dir(name, Draft202012Validator(schema), surprises)
    if not schemas:
        surprises.append("no *.schema.json in contracts/")
    print(f"contracts: {len(schemas)} schemas, {count} examples, {len(surprises)} surprises")
    for s in surprises:
        print(f"  {s}")
    return 1 if surprises else 0


if __name__ == "__main__":
    sys.exit(main())
