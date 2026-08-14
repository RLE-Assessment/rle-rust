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
from typing import Any, Literal, Sequence

from ._iucn_rle import criterion_b_json, thresholds_sha256, thresholds_toml, version

__all__ = [
    "__version__",
    "criterion_b",
    "thresholds_sha256",
    "thresholds_toml",
    "version",
]

__version__ = version()

Status = Literal["met", "not_met", "not_assessed"]


def criterion_b(
    eoo_km2: float | None = None,
    aoo_cells: float | None = None,
    subconditions: dict[str, Status] | Sequence[tuple[str, Status]] | None = None,
    *,
    eoo_bounds: tuple[float, float] | None = None,
    aoo_bounds: tuple[float, float] | None = None,
) -> dict[str, Any]:
    """Assess IUCN RLE Criterion B (restricted geographic distribution).

    Args:
        eoo_km2: Extent of occurrence in km2 (sub-criterion B1).
        aoo_cells: Occupied 10x10 km cells after the 1% exclusion (B2).
        subconditions: Status of sub-conditions ``"a"``, ``"b"``, and ``"c"``,
            each ``"met"``, ``"not_met"``, or ``"not_assessed"``. Anything
            omitted counts as *not assessed*, which is deliberately different
            from *not met*.
        eoo_bounds: Plausible ``(lower, upper)`` bounds on the EOO.
        aoo_bounds: Plausible ``(lower, upper)`` bounds on the AOO.

    Returns:
        A dict with the overall category, each sub-criterion, any caveats, and
        the provenance needed to reproduce the result.

    The spatial thresholds alone do not produce a listing: Criterion B also
    requires at least one sub-condition. So an ecosystem whose metrics say EN,
    but whose sub-conditions nobody has examined, is reported as ``"EN (LC-EN)"``
    rather than a bare ``"EN"`` that would overstate confidence::

        >>> iucn_rle.criterion_b(eoo_km2=15_000)["overall"]
        'EN (LC-EN)'
        >>> iucn_rle.criterion_b(eoo_km2=15_000, subconditions={"a": "met"})["overall"]
        'EN'
    """
    if isinstance(subconditions, dict):
        pairs = list(subconditions.items())
    else:
        pairs = list(subconditions or [])

    return json.loads(
        criterion_b_json(
            eoo_km2=eoo_km2,
            aoo_cells=aoo_cells,
            subconditions=pairs,
            eoo_lower_km2=eoo_bounds[0] if eoo_bounds else None,
            eoo_upper_km2=eoo_bounds[1] if eoo_bounds else None,
            aoo_lower_cells=aoo_bounds[0] if aoo_bounds else None,
            aoo_upper_cells=aoo_bounds[1] if aoo_bounds else None,
        )
    )
