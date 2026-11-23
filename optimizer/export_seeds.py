"""Export the optimizer database's 20 highest-scoring results as JSON."""

import argparse
import json
import re
import sqlite3
from pathlib import Path


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
        return "BoostDecay", {
            name: str(int(value) * 10**14)
            for name, value in zip(names, fields.groups())
        }

    values = json.loads(stored_params)["values"]
    return template_name, {
        name: definition["generated_value"]
        for name, definition in values.items()
        if definition["generated_value"] is not None
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
            LIMIT 20
            """
        ).fetchall()

    if len(rows) != 20:
        parser.error(f"expected at least 20 results, found {len(rows)}")

    seeds = []
    for result_id, stored_template, stored_params, edge in rows:
        template_name, params = seed_params(stored_template, stored_params)
        seeds.append(
            {
                "result_id": result_id,
                "template_name": template_name,
                "recorded_edge": edge,
                "params": params,
            }
        )
    args.output.write_text(
        json.dumps(seeds, indent=2, sort_keys=True, allow_nan=False) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
