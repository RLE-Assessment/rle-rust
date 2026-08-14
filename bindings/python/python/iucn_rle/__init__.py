"""IUCN Red List of Ecosystems assessment calculations, powered by Rust.

The public API is deliberately synchronous. Long-running functions release the GIL
for the whole computation, so notebook users can keep the event loop responsive with
the standard library rather than a bespoke async API::

    grid = await asyncio.to_thread(iucn_rle.aoo_grid, url)

This package is independent of ``rle-python``; it does not share the ``rle`` namespace.
"""

from ._iucn_rle import version

__all__ = ["__version__", "version"]

__version__ = version()
