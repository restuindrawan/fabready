# Parts review

fabready checks each line of the bill of materials against what the board puts on it. You'll find the
results in the app's **Parts** tab, from `fabready parts <project>`, and from the `review_parts` MCP tool.

## Verdicts

- **Under-rated:** the part is rated below what the board puts on it. Examples are a 6.3 V capacitor on a
  5 V rail, an LED driven too hard by its resistor, a regulator below its dropout or too small for its load,
  or a part powered outside its supply range. These also appear as findings (`part.*`).
- **Over-spec:** more part than the job needs, such as a 1206 pull-up, a 50 V 1206 capacitor on 3.3 V, a
  through-hole rectifier on a signal line, or a 1 A regulator for a 30 mA load. These are notes, not findings.
- **OK:** rated for what it sees.
- **Unknown:** fabready couldn't find the part's rating or the voltages around it.
- **Not rated:** connectors, switches and other parts fabready doesn't judge.

`fabready parts` exits with 1 when any part is under-rated.

## Where the numbers come from

Ratings, in order of preference:

1. The part's value and description fields, such as `10uF 16V` or `0.25W`
2. A built-in library of common parts, with typical datasheet values
3. Typical limits for the package (for example, a 0603 resistor's power rating)

What the board puts on each part:

- **Rail voltages** come from net names (`+3V3`, `+5V`, `VBUS`, `BAT+`…), from regulator outputs, and from
  `power.rails` in `fabready.json`.
- **Regulator loads** come from `power.current_ma` in `fabready.json`.
- **LED current** is worked out from the series resistor and the rail.

Where one side of a part has an unknown voltage, fabready assumes the highest rail on the board and marks
the value "(max)".

## Improving the results

- Put ratings in the value or description field ("10uF 16V") for parts the library doesn't know.
- Name power nets clearly, or set them in `power.rails` (see [configuration.md](configuration.md#power)).
- Set expected loads in `power.current_ma` so regulators can be sized.
- For **Unknown** parts, ask your agent: `review_parts` gives it everything fabready knows, and it can judge
  the rest from the datasheet.

All of this runs offline. fabready doesn't look parts up online. Library values are typical, so check the
datasheet for anything critical.
