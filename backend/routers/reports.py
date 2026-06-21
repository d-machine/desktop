import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends
from pydantic import BaseModel

from routers.deps import get_conn
from services.reports import export_tax_report, get_capital_gains, get_income

router = APIRouter(tags=["reports"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class ReportFilter(BaseModel):
    fy: str | None = None
    account_ids: list[int] | None = None


class ExportInput(BaseModel):
    fy: str
    dest_path: str


@router.post("/capital-gains")
def capital_gains(body: ReportFilter, conn: Conn):
    return get_capital_gains(conn, fy=body.fy, account_ids=body.account_ids)


@router.post("/income")
def income(body: ReportFilter, conn: Conn):
    return get_income(conn, fy=body.fy, account_ids=body.account_ids)


@router.post("/export-tax")
def export_tax(body: ExportInput, conn: Conn):
    export_tax_report(conn, body.fy, body.dest_path)
    return {"ok": True}
