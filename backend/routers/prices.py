import sqlite3
import logging
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn
from services import prices as prices_service

router = APIRouter(tags=["prices"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]
logger = logging.getLogger("uvicorn.error")


class SyncPricesInput(BaseModel):
    force: bool = False


@router.post("/resolve-instruments")
def resolve_instruments(conn: Conn):
    base_url = prices_service.get_server_url(conn)
    try:
        logger.info("POST /api/prices/resolve-instruments -> %s", base_url)
        return prices_service.resolve_instruments(conn)
    except Exception as e:
        logger.exception("Instrument resolution failed via %s", base_url)
        raise HTTPException(status_code=502, detail=f"Instrument resolution failed via {base_url}: {e}")


@router.post("/sync")
def sync_prices(body: SyncPricesInput, conn: Conn):
    base_url = prices_service.get_server_url(conn)
    try:
        logger.info("POST /api/prices/sync -> %s", base_url)
        return prices_service.sync_prices(conn, force=body.force)
    except Exception as e:
        logger.exception("Price sync failed via %s", base_url)
        raise HTTPException(status_code=502, detail=f"Price sync failed via {base_url}: {e}")
