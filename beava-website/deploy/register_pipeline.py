"""Pipeline registered on beava.dev — the canonical SiteMetrics shape.

This file IS the example shown on https://beava.dev. To change what
beava.dev computes, edit this file (and the inline example in
`beava-website/project/index.html` to keep them in sync) and merge to
main; the deploy workflow regenerates the wire payload via this Python
SDK and POSTs it to the live `/register` endpoint with `force=true`.

Usage:
  python register_pipeline.py --dump                # print wire JSON to stdout
  python register_pipeline.py http://beava:8080      # register against server
"""
from __future__ import annotations

import sys

import beava as bv


@bv.event
class PageView:
    session_id: str
    path: str
    dwell_ms: int  # set when the visitor leaves the page


@bv.table  # no key= → one row, site-wide (ADR-003)
def SiteMetrics(e: PageView):
    return e.agg(
        median_dwell_1h=bv.quantile("dwell_ms", q=0.5, window="1h"),
        page_views_24h=bv.count(window="24h"),
        top_page_1h=bv.top_k("path", k=1, window="1h"),
    )


def _dump_payload() -> bytes:
    """Render the wire-shape JSON the server expects, with force=true."""
    # `_to_register_json` is the SDK's private renderer (App.register uses it
    # to build the POST body). Acceptable boundary-crossing for a deploy
    # script that needs the bytes without an actual transport.
    from beava._app import _to_register_json

    return _to_register_json((PageView, SiteMetrics), force=True)


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: register_pipeline.py {--dump | <server-url>}", file=sys.stderr)
        return 2
    arg = sys.argv[1]
    if arg == "--dump":
        sys.stdout.buffer.write(_dump_payload())
        return 0
    with bv.App(arg) as app:
        app.register(PageView, SiteMetrics, force=True)
    print(f"OK: SiteMetrics + PageView registered at {arg}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
