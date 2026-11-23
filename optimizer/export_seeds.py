"""Export the top 20 optimizer results plus two from each remaining strategy type."""

import argparse
import json
import re
import sqlite3
from pathlib import Path


TEMPLATE_ALIASES = {"boost_decay_2": "BoostDecay"}


def seed_params(template_name, stored_params):
    if template_name == "boost_decay_2":
        # The legacy template multiplied these five integers by Solidity's BPS (1e14).
        fields = re.fullmatch(
            r"base_fee=([0-9]+)bps\|decay_fast=([0-9]+)bps\|"
            r"decay_slow=([0-9]+)bps\|decay_cooldown=([0-9]+)bps\|"
            r"alpha_spot=([0-9]+)bps",
            stored_params,
        )
        if fields is None:
            raise ValueError(f"Invalid legacy BoostDecay parameters: {stored_params}")
        names = ("BASE_FEE", "DECAY_FAST", "DECAY_SLOW", "DECAY_COOLDOWN", "ALPHA_SPOT")
        return {
            name: str(int(value) * 10**14)
            for name, value in zip(names, fields.groups())
        }

    values = json.loads(stored_params)["values"]
    return {
        name: definition["generated_value"]
        for name, definition in values.items()
        if definition["generated_value"] is not None
    }


def make_seed(row):
    result_id, template_name, stored_params, edge = row
    return {
        "result_id": result_id,
        "template_name": TEMPLATE_ALIASES.get(template_name, template_name),
        "recorded_edge": edge,
        "params": seed_params(template_name, stored_params),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    database = args.database.resolve()
    if database == args.output.resolve():
        parser.error("the seed file must be separate from the source database")
    database_url = database.as_uri() + "?mode=ro"
    with sqlite3.connect(database_url, uri=True) as connection:
        rows = connection.execute(
            """
            SELECT id, template_name, params, edge
            FROM strategy_results
            ORDER BY edge DESC, id ASC
            """
        ).fetchall()

    if len(rows) < 20:
        parser.error(f"expected at least 20 results, found {len(rows)}")

    seeds = [make_seed(row) for row in rows[:20]]
    top_templates = {seed["template_name"] for seed in seeds}
    supplemental_params = {}
    for row in rows[20:]:
        stored_template = row[1]
        template_name = TEMPLATE_ALIASES.get(stored_template, stored_template)
        if template_name in top_templates:
            continue
        selected = supplemental_params.setdefault(template_name, set())
        if len(selected) == 2:
            continue
        seed = make_seed(row)
        params = tuple(sorted(seed["params"].items()))
        if params not in selected:
            selected.add(params)
            seeds.append(seed)

    args.output.write_text(
        json.dumps(seeds, indent=2, sort_keys=True, allow_nan=False) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
