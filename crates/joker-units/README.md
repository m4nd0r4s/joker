# joker-units

Units, dimensions and quantities of the v1 catalogue (docs/16 §5), with the literal grammars of ref/00 §3. `no_std`, no allocation, no dependencies.

- [`Unit`]: an atom of the catalogue, prefixed (`µ m k M G`) or combined into a compound (`kW/s`, `m3/h/%`, `°C/min`), reduced to a [`Dimension`], a factor and an offset to SI, and an optional [`QuantityKind`].
- [`Quantity`] and [`Literal`]: quantity literals such as `"125 kW"` or `"-20 kW/s"`.
- [`Duration`]: duration literals such as `"200ms"` or `"1.5 s"`, and their canonical form.
- [`Error`]: every failure carries a [`Span`] and maps to a registry code (`J0107`, `J0108`, `J0109`, `J0151`, `J0152`).

`%` is its own dimension axis, so it never mixes with `1` (docs/16 §4); [`Unit::as_fraction`] gives the explicit conversion. `°C` is affine: alone it is an absolute temperature, inside a compound it is a difference.
