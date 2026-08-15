"""IUCN Red List of Ecosystems assessment calculations, powered by Rust.

The public API is deliberately synchronous. Long-running functions release the GIL
for the whole computation, so notebook users can keep the event loop responsive
with the standard library rather than a bespoke async API::

    grid = await asyncio.to_thread(iucn_rle.aoo_grid, url)

This package is independent of ``rle-python``; it does not share the ``rle``
namespace.
"""

from __future__ import annotations

import json
from typing import Any, Literal, Mapping, Sequence

from ._iucn_rle import (
    criterion_b_json,
    distribution_metrics_json,
    thresholds_sha256,
    thresholds_toml,
    version,
)

__all__ = [
    "__version__",
    "criterion_b",
    "distribution_metrics",
    "thresholds_sha256",
    "thresholds_toml",
    "version",
]

__version__ = version()

Rings = Sequence[Sequence[Sequence[float]]]


def distribution_metrics(
    polygons: Sequence[tuple[str, Rings]] | Mapping[str, Sequence[Rings]],
) -> dict[str, Any]:
    """Compute Criterion B spatial metrics from a distribution map.

    Args:
        polygons: Either a sequence of ``(ecosystem, rings)`` pairs, or a mapping of
            ecosystem to a list of that ecosystem's polygons. ``rings`` is the
            exterior ring followed by any holes, each a sequence of ``[lon, lat]``
            pairs in **degrees on WGS84**.

    Returns:
        A dict with one entry per ecosystem giving ``eoo_km2`` and ``aoo_cells``,
        plus the grid CRS and cell size for provenance.

    A ring's role is decided by its **position**, not its winding: ``rings[0]`` is
    the exterior and the rest are holes, whichever way each one winds. Source formats
    disagree — GeoJSON specifies counter-clockwise exteriors, shapefiles the
    opposite, and real files often follow neither — so the format cannot change an
    assessment.

    Coordinates outside valid longitude/latitude range raise rather than being
    clamped, because they almost always mean the values are swapped or already
    projected, and silently coping would produce a plausible but wrong AOO.

        >>> square = [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]]
        >>> result = iucn_rle.distribution_metrics([("T1.1.1", square)])
        >>> round(result["ecosystems"][0]["eoo_km2"])
        12309

    The result feeds straight into :func:`criterion_b`.
    """
    if isinstance(polygons, Mapping):
        pairs = [
            (ecosystem, rings)
            for ecosystem, features in polygons.items()
            for rings in features
        ]
    else:
        pairs = list(polygons)

    return json.loads(distribution_metrics_json(pairs))

Status = Literal["met", "not_met", "not_assessed"]


def criterion_b(
    eoo_km2: float | None = None,
    aoo_cells: float | None = None,
    clauses: dict[str, Status] | Sequence[tuple[str, Status]] | None = None,
    *,
    locations: int | None = None,
    no_plausible_threats: bool = False,
    locations_insufficient_information: bool = False,
    eoo_bounds: tuple[float, float] | None = None,
    aoo_bounds: tuple[float, float] | None = None,
) -> dict[str, Any]:
    """Assess IUCN RLE Criterion B (restricted geographic distribution).

    Args:
        eoo_km2: Extent of occurrence in km2 (sub-criterion B1).
        aoo_cells: Occupied 10x10 km cells after the 1% exclusion (B2).
        clauses: Status of clause ``"a"`` (or its aspects ``"a.i"``, ``"a.ii"``,
            ``"a.iii"``), clause ``"b"``, and ``"b3_rapid_collapse"`` for B3's
            second limb. Each is ``"met"``, ``"not_met"`` or ``"not_assessed"``.
            Anything omitted counts as *not assessed*, which is deliberately
            different from *not met*.
        locations: Clause (c) — the number of threat-defined locations. This is a
            **count, not a status**, because clause (c) is category dependent:
            1 location for CR, <=5 for EN, <=10 for VU.
        no_plausible_threats: No plausible threats exist, so clause (c) and B3 are
            *not met*. A finding, distinct from omitting ``locations``.
        locations_insufficient_information: Threats exist but their extent cannot
            be assessed, yielding Data Deficient.
        eoo_bounds: Plausible ``(lower, upper)`` bounds on the EOO.
        aoo_bounds: Plausible ``(lower, upper)`` bounds on the AOO.

    Returns:
        A dict with the overall category, each sub-criterion, any caveats, and
        the provenance needed to reproduce the result.

    The spatial thresholds alone do not produce a listing: Criterion B also
    requires at least one of clauses (a), (b) or (c). So an ecosystem whose
    metrics say EN, but whose clauses nobody has examined, is reported as
    ``"EN (LC-EN)"`` rather than a bare ``"EN"`` that would overstate confidence::

        >>> iucn_rle.criterion_b(eoo_km2=15_000)["overall"]
        'EN (LC-EN)'
        >>> iucn_rle.criterion_b(eoo_km2=15_000, clauses={"a": "met"})["overall"]
        'EN'

    Clause (c) being category dependent has teeth. An EOO of 1,500 km2 is in the
    CR band, but CR requires exactly one threat-defined location, so three
    locations qualifies only at EN::

        >>> iucn_rle.criterion_b(
        ...     eoo_km2=1_500,
        ...     clauses={"a": "not_met", "b": "not_met"},
        ...     locations=3,
        ... )["criteria"][0]["category"]
        'EN'
    """
    if isinstance(clauses, dict):
        pairs = list(clauses.items())
    else:
        pairs = list(clauses or [])

    return json.loads(
        criterion_b_json(
            eoo_km2=eoo_km2,
            aoo_cells=aoo_cells,
            clauses=pairs,
            locations=locations,
            no_plausible_threats=no_plausible_threats,
            locations_insufficient_information=locations_insufficient_information,
            eoo_lower_km2=eoo_bounds[0] if eoo_bounds else None,
            eoo_upper_km2=eoo_bounds[1] if eoo_bounds else None,
            aoo_lower_cells=aoo_bounds[0] if aoo_bounds else None,
            aoo_upper_cells=aoo_bounds[1] if aoo_bounds else None,
        )
    )
