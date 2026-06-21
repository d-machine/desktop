import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends
from pydantic import BaseModel

from routers.deps import get_conn
from services.holdings import compute_holdings, compute_portfolio_summary

router = APIRouter(tags=["holdings"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class HoldingsFilter(BaseModel):
    account_ids: list[int] | None = None
    portfolio_ids: list[int] | None = None
    asset_classes: list[str] | None = None


@router.post("")
def get_holdings(body: HoldingsFilter, conn: Conn):
    return compute_holdings(conn, body.account_ids, body.portfolio_ids, body.asset_classes)


@router.post("/summary")
def get_portfolio_summary(body: HoldingsFilter, conn: Conn):
    return compute_portfolio_summary(conn, body.account_ids, body.portfolio_ids, body.asset_classes)
