import { useEffect, useState, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useReactTable, getCoreRowModel, flexRender,
  type ColumnDef,
} from "@tanstack/react-table";
import { Plus, Upload, Trash2, ArrowUpDown, ArrowUp, ArrowDown, Pencil, AlertTriangle, CheckCircle2, X, FileText, ChevronLeft, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Sheet, SheetContent, SheetHeader, SheetTitle,
} from "@/components/ui/sheet";
import {
  Tooltip, TooltipContent, TooltipProvider, TooltipTrigger,
} from "@/components/ui/tooltip";
import { openPath } from "@tauri-apps/plugin-opener";
import { AddTransactionDialog } from "@/components/transactions/AddTransactionDialog";
import { EditTransactionDialog } from "@/components/transactions/EditTransactionDialog";
import { ImportDialog } from "@/components/transactions/ImportDialog";
import { formatINR, formatQty, formatDate } from "@/lib/format";
import { TXN_TYPE_COLORS } from "@/lib/txn-types";
import { cn } from "@/lib/utils";

interface Transaction {
  txn_id: number;
  account_id: number;
  account_name: string;
  instrument_id: number;
  instrument_name: string;
  isin?: string;
  txn_type: string;
  trade_segment: string;
  trade_date: string;
  txn_time?: string;
  quantity: number;
  price_paise: number;
  total_value_paise: number;
  brokerage_paise: number;
  stt_paise: number;
  other_charges_paise: number;
  notes?: string;
  broker_ref?: string;
  flag?: string;
  flag_reason?: string;
  flag_dismissed: boolean;
  batch_id?: number;
}

interface ImportBatch {
  batch_id: number;
  source_type: string;
  file_name?: string;
  ref_no?: string;
  broker?: string;
  batch_trade_date?: string;
  imported_at: string;
  record_count: number;
  stt_paise: number;
  stamp_charges_paise: number;
  gst_paise: number;
  trans_charges_paise: number;
  other_charges_paise: number;
  total_payable_paise: number;
  transactions: Transaction[];
}

interface Portfolio { portfolio_id: number; name: string; }
interface Account { account_id: number; portfolio_id: number; name: string; account_type: string; broker?: string; }

type FlagFilter = "all" | "flagged" | "clean";
type SortDir = "asc" | "desc";

const SORTABLE_COLS = ["trade_date", "instrument_name", "quantity", "price_paise", "total_value_paise"] as const;
type SortCol = typeof SORTABLE_COLS[number];

export function TransactionsPage() {
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [accounts, setAccounts]         = useState<Account[]>([]);
  const [portfolios, setPortfolios]     = useState<Portfolio[]>([]);
  const [loading, setLoading] = useState(true);
  const [reEvaluating, setReEvaluating] = useState(false);
  const [batchDetail, setBatchDetail]   = useState<ImportBatch | null>(null);
  const [batchLoading, setBatchLoading] = useState(false);

  const [showAdd, setShowAdd]       = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Transaction | null>(null);
  const [editTarget, setEditTarget]     = useState<Transaction | null>(null);

  // Filters
  const [filterPortfolio, setFilterPortfolio] = useState<string>("all");
  const [filterAccount, setFilterAccount]     = useState<string>("all");
  const [flagFilter, setFlagFilter]           = useState<FlagFilter>("all");

  // Pagination
  const [page, setPage]           = useState(1);
  const [pageSize, setPageSize]   = useState(100);
  const [totalCount, setTotalCount] = useState(0);
  const [flaggedTotal, setFlaggedTotal] = useState(0);

  // Sorting
  const [sortCol, setSortCol] = useState<SortCol>("trade_date");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  // Search — raw input debounced to avoid DB hit on every keystroke
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch]           = useState("");

  // Reload trigger — increment to force a reload without changing other state
  const [loadTick, setLoadTick] = useState(0);
  const reload = useCallback(() => setLoadTick(t => t + 1), []);

  const visibleAccounts = useMemo(() =>
    filterPortfolio === "all"
      ? accounts
      : accounts.filter(a => a.portfolio_id.toString() === filterPortfolio),
    [accounts, filterPortfolio]
  );

  const activeAccountIds = useMemo((): number[] | undefined => {
    if (filterAccount !== "all") return [parseInt(filterAccount)];
    if (filterPortfolio !== "all") return visibleAccounts.map(a => a.account_id);
    return undefined;
  }, [filterAccount, filterPortfolio, visibleAccounts]);

  // Debounce search input
  useEffect(() => {
    const t = setTimeout(() => { setSearch(searchInput); setPage(1); }, 300);
    return () => clearTimeout(t);
  }, [searchInput]);

  // Main data load
  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    const filter = {
      ...(activeAccountIds ? { account_ids: activeAccountIds } : {}),
      ...(flagFilter !== "all" ? { flag_filter: flagFilter } : {}),
      ...(search ? { search } : {}),
      sort_col: sortCol,
      sort_dir: sortDir,
      limit: pageSize,
      offset: (page - 1) * pageSize,
    };

    Promise.all([
      invoke<Transaction[]>("get_transactions", { filter }),
      invoke<number>("get_transactions_count", { filter }),
      invoke<number>("get_flagged_count", { accountIds: activeAccountIds ?? null }),
    ]).then(([txns, count, flagged]) => {
      if (cancelled) return;
      setTransactions(txns);
      setTotalCount(count);
      setFlaggedTotal(flagged);
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });

    return () => { cancelled = true; };
  }, [activeAccountIds, flagFilter, search, sortCol, sortDir, page, pageSize, loadTick]);

  // Initial meta load
  useEffect(() => {
    Promise.all([
      invoke<Portfolio[]>("get_portfolios"),
      invoke<Account[]>("get_accounts", { portfolioId: null }),
    ]).then(([ps, as_]) => { setPortfolios(ps); setAccounts(as_); });
  }, []);

  const handleSort = useCallback((col: SortCol) => {
    if (col === sortCol) {
      setSortDir(d => d === "asc" ? "desc" : "asc");
    } else {
      setSortCol(col);
      setSortDir("desc");
    }
    setPage(1);
  }, [sortCol]);

  const handleReEvaluate = async () => {
    setReEvaluating(true);
    try {
      await invoke("re_evaluate_flags", { accountId: null });
      reload();
    } finally {
      setReEvaluating(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    await invoke("delete_transaction", { txnId: deleteTarget.txn_id });
    setDeleteTarget(null);
    reload();
  };

  const handleDismissFlag = async (txn: Transaction) => {
    await invoke("dismiss_transaction_flag", { txnId: txn.txn_id });
    reload();
  };

  const openBatch = async (batchId: number) => {
    setBatchLoading(true);
    setBatchDetail(null);
    try {
      const b = await invoke<ImportBatch>("get_import_batch", { batchId });
      setBatchDetail(b);
    } finally {
      setBatchLoading(false);
    }
  };

  const totalPages = Math.max(1, Math.ceil(totalCount / pageSize));

  const columns = useMemo<ColumnDef<Transaction>[]>(() => [
    {
      id: "flag",
      header: "",
      cell: ({ row }) => {
        const t = row.original;
        return (
          <div className="flex items-center gap-1">
            {t.flag && !t.flag_dismissed && (
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger>
                    <AlertTriangle className="size-3.5 text-amber-500" />
                  </TooltipTrigger>
                  <TooltipContent side="right" className="max-w-[260px]">
                    <p className="font-medium text-amber-600">{t.flag}</p>
                    {t.flag_reason && <p className="text-xs mt-0.5 text-muted-foreground">{t.flag_reason}</p>}
                    <p className="text-xs mt-1 text-muted-foreground">Excluded from portfolio. Edit or dismiss to include.</p>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            )}
            {t.batch_id != null && (
              <FileText className="size-3.5 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
            )}
          </div>
        );
      },
      size: 40,
    },
    {
      id: "trade_date",
      header: () => <SortHeader colId="trade_date" label="Date" sortCol={sortCol} sortDir={sortDir} onSort={handleSort} />,
      cell: ({ row }) => (
        <span className={cn("text-sm tabular-nums", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {formatDate(row.original.trade_date)}
        </span>
      ),
      size: 100,
    },
    {
      id: "instrument_name",
      header: () => <SortHeader colId="instrument_name" label="Instrument" sortCol={sortCol} sortDir={sortDir} onSort={handleSort} />,
      cell: ({ row }) => (
        <div className={cn(row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          <div className="text-sm font-medium truncate max-w-[200px]">{row.original.instrument_name}</div>
          {row.original.isin && <div className="text-xs text-muted-foreground font-mono">{row.original.isin}</div>}
        </div>
      ),
    },
    {
      id: "txn_type",
      header: "Type",
      cell: ({ row }) => (
        <div className={cn("flex flex-col gap-0.5", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          <span className={cn("text-sm font-medium", TXN_TYPE_COLORS[row.original.txn_type] ?? "")}>
            {row.original.txn_type}
          </span>
          {row.original.trade_segment !== "DELIVERY" && (
            <Badge variant="outline" className="text-xs w-fit px-1 py-0">{row.original.trade_segment}</Badge>
          )}
        </div>
      ),
      size: 90,
    },
    {
      id: "account_name",
      header: "Account",
      cell: ({ row }) => (
        <span className={cn("text-sm text-muted-foreground", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {row.original.account_name}
        </span>
      ),
    },
    {
      id: "quantity",
      header: () => <SortHeader colId="quantity" label="Qty" right sortCol={sortCol} sortDir={sortDir} onSort={handleSort} />,
      cell: ({ row }) => (
        <span className={cn("text-sm tabular-nums text-right block", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {formatQty(row.original.quantity)}
        </span>
      ),
      size: 80,
    },
    {
      id: "price_paise",
      header: () => <SortHeader colId="price_paise" label="Price" right sortCol={sortCol} sortDir={sortDir} onSort={handleSort} />,
      cell: ({ row }) => (
        <span className={cn("text-sm tabular-nums text-right block", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {formatINR(row.original.price_paise)}
        </span>
      ),
      size: 110,
    },
    {
      id: "total_value_paise",
      header: () => <SortHeader colId="total_value_paise" label="Total" right sortCol={sortCol} sortDir={sortDir} onSort={handleSort} />,
      cell: ({ row }) => {
        const val = row.original.total_value_paise;
        return (
          <span className={cn(
            "text-sm tabular-nums text-right block font-medium",
            row.original.flag && !row.original.flag_dismissed
              ? "opacity-50"
              : val > 0 ? "text-green-600 dark:text-green-400"
              : val < 0 ? "text-red-600 dark:text-red-400"
              : ""
          )}>
            {val < 0 ? "−" : val > 0 ? "+" : ""}₹{Math.abs(val / 100).toLocaleString("en-IN", { minimumFractionDigits: 2 })}
          </span>
        );
      },
      size: 120,
    },
    {
      id: "actions",
      header: "",
      cell: ({ row }) => {
        const t = row.original;
        const isFlagged = t.flag && !t.flag_dismissed;
        return (
          <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
            {isFlagged && (
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger>
                    <button
                      className="p-1 text-muted-foreground hover:text-green-600 transition-colors"
                      onClick={() => handleDismissFlag(t)}
                    >
                      <CheckCircle2 className="size-3.5" />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent>Dismiss flag — include in portfolio</TooltipContent>
                </Tooltip>
              </TooltipProvider>
            )}
            <button
              className="p-1 text-muted-foreground hover:text-foreground transition-colors"
              onClick={() => setEditTarget(t)}
            >
              <Pencil className="size-3.5" />
            </button>
            <button
              className="p-1 text-muted-foreground hover:text-destructive transition-colors"
              onClick={() => setDeleteTarget(t)}
            >
              <Trash2 className="size-3.5" />
            </button>
          </div>
        );
      },
      size: 80,
    },
  ], [sortCol, sortDir, handleSort]);

  const table = useReactTable({
    data: transactions,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Transactions</h1>
          <p className="text-sm text-muted-foreground">
            {totalCount.toLocaleString()} transaction{totalCount !== 1 ? "s" : ""}
            {flaggedTotal > 0 && (
              <span className="ml-2 text-amber-600 font-medium">
                · {flaggedTotal} issue{flaggedTotal !== 1 ? "s" : ""}
              </span>
            )}
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" onClick={handleReEvaluate} disabled={reEvaluating}>
            <RefreshCw className={`size-4 mr-2 ${reEvaluating ? "animate-spin" : ""}`} />
            {reEvaluating ? "Checking…" : "Re-evaluate"}
          </Button>
          <Button variant="outline" onClick={() => setShowImport(true)}>
            <Upload className="size-4 mr-2" /> Import
          </Button>
          <Button onClick={() => setShowAdd(true)}>
            <Plus className="size-4 mr-2" /> Add
          </Button>
        </div>
      </div>

      {/* Filters */}
      <div className="flex gap-2 flex-wrap items-center">
        <Input
          placeholder="Search instrument, account, type…"
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          className="w-64 h-8 text-xs"
        />
        <Select value={filterPortfolio} onValueChange={(v) => { setFilterPortfolio(v ?? "all"); setFilterAccount("all"); setPage(1); }}>
          <SelectTrigger className="w-40 h-8 text-xs">
            <SelectValue placeholder="ALL PORTFOLIOS" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">ALL PORTFOLIOS</SelectItem>
            {portfolios.map(p => (
              <SelectItem key={p.portfolio_id} value={p.portfolio_id.toString()}>{p.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={filterAccount} onValueChange={(v) => { setFilterAccount(v ?? "all"); setPage(1); }} disabled={visibleAccounts.length === 0}>
          <SelectTrigger className="w-40 h-8 text-xs">
            <SelectValue placeholder="ALL ACCOUNTS" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">ALL ACCOUNTS</SelectItem>
            {visibleAccounts.map(a => (
              <SelectItem key={a.account_id} value={a.account_id.toString()}>
                {a.name}{a.broker ? ` · ${a.broker}` : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        {/* Flag filter tabs */}
        <div className="flex rounded-md border overflow-hidden h-8 ml-auto">
          {(["all", "flagged", "clean"] as FlagFilter[]).map((f) => (
            <button
              key={f}
              onClick={() => { setFlagFilter(f); setPage(1); }}
              className={cn(
                "px-3 text-xs font-medium border-r last:border-r-0 transition-colors",
                flagFilter === f
                  ? "bg-foreground text-background"
                  : "bg-background text-muted-foreground hover:bg-muted"
              )}
            >
              {f === "all" ? "All" : f === "flagged" ? (
                <span className="flex items-center gap-1">
                  <AlertTriangle className="size-3 text-amber-500" /> Issues
                </span>
              ) : (
                <span className="flex items-center gap-1">
                  <X className="size-3" /> Clean
                </span>
              )}
            </button>
          ))}
        </div>
      </div>

      {/* Table */}
      <div className="flex-1 border rounded-lg overflow-hidden">
        <div className="overflow-auto h-full">
          <table className="w-full text-sm">
            <thead className="bg-muted sticky top-0 z-10">
              {table.getHeaderGroups().map((hg) => (
                <tr key={hg.id}>
                  {hg.headers.map((header) => (
                    <th
                      key={header.id}
                      className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground whitespace-nowrap"
                      style={{ width: header.getSize() }}
                    >
                      {flexRender(header.column.columnDef.header, header.getContext())}
                    </th>
                  ))}
                </tr>
              ))}
            </thead>
            <tbody className="divide-y">
              {loading ? (
                <tr><td colSpan={columns.length} className="px-3 py-8 text-center text-muted-foreground text-sm">Loading…</td></tr>
              ) : table.getRowModel().rows.length === 0 ? (
                <tr>
                  <td colSpan={columns.length} className="px-3 py-16 text-center">
                    {search || flagFilter !== "all" ? (
                      <p className="text-muted-foreground text-sm">No transactions match your filter.</p>
                    ) : (
                      <div className="flex flex-col items-center gap-4">
                        <div className="flex flex-col items-center gap-1">
                          <p className="text-base font-medium text-foreground">No transactions yet</p>
                          <p className="text-sm text-muted-foreground">Import a broker statement or add a transaction manually.</p>
                        </div>
                        <div className="flex gap-3">
                          <Button size="lg" variant="outline" onClick={() => setShowImport(true)}>
                            <Upload className="size-5 mr-2" /> Import Statement
                          </Button>
                          <Button size="lg" onClick={() => setShowAdd(true)}>
                            <Plus className="size-5 mr-2" /> Add Transaction
                          </Button>
                        </div>
                      </div>
                    )}
                  </td>
                </tr>
              ) : (
                table.getRowModel().rows.map((row) => {
                  const hasBatch = row.original.batch_id != null;
                  return (
                    <tr
                      key={row.id}
                      className={cn(
                        "group hover:bg-muted/30 transition-colors",
                        hasBatch && "cursor-pointer",
                        row.original.flag && !row.original.flag_dismissed && "bg-amber-50/30 dark:bg-amber-900/10"
                      )}
                      onClick={hasBatch ? () => openBatch(row.original.batch_id!) : undefined}
                    >
                      {row.getVisibleCells().map((cell) => (
                        <td
                          key={cell.id}
                          className="px-3 py-2"
                          onClick={cell.column.id === "actions" ? (e) => e.stopPropagation() : undefined}
                        >
                          {flexRender(cell.column.columnDef.cell, cell.getContext())}
                        </td>
                      ))}
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Pagination */}
      <div className="flex items-center justify-between py-1 text-sm shrink-0">
        <span className="text-xs text-muted-foreground">
          {totalCount > 0 && (
            <>
              Showing {((page - 1) * pageSize + 1).toLocaleString()}–{Math.min(page * pageSize, totalCount).toLocaleString()} of {totalCount.toLocaleString()}
            </>
          )}
        </span>
        <div className="flex items-center gap-2">
          <Button
            variant="outline" size="sm"
            disabled={page <= 1 || loading}
            onClick={() => setPage(p => p - 1)}
          >
            ← Prev
          </Button>
          <span className="text-xs text-muted-foreground tabular-nums">
            Page {page} of {totalPages}
          </span>
          <Button
            variant="outline" size="sm"
            disabled={page >= totalPages || loading}
            onClick={() => setPage(p => p + 1)}
          >
            Next →
          </Button>
          <Select value={pageSize.toString()} onValueChange={(v) => { if (v) { setPageSize(parseInt(v)); setPage(1); } }}>
            <SelectTrigger className="w-20 h-8 text-xs"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="50">50 / page</SelectItem>
              <SelectItem value="100">100 / page</SelectItem>
              <SelectItem value="250">250 / page</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Dialogs */}
      <AddTransactionDialog
        open={showAdd}
        onOpenChange={setShowAdd}
        onSaved={() => {
          reload();
          invoke("resolve_instruments")
            .catch(() => {})
            .then(() => invoke("sync_prices", { force: true }).catch(() => {}));
        }}
      />
      <EditTransactionDialog
        open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)}
        transaction={editTarget}
        onSaved={reload}
      />
      <ImportDialog
        open={showImport}
        onOpenChange={setShowImport}
        onImported={() => {
          invoke<Account[]>("get_accounts", { portfolioId: null }).then(setAccounts);
          invoke("re_evaluate_flags", { accountId: null }).catch(() => {}).then(reload);
          invoke("resolve_instruments")
            .catch(() => {})
            .then(() => invoke("sync_prices", { force: true }).catch(() => {}));
        }}
      />
      <Sheet open={batchDetail !== null || batchLoading} onOpenChange={(o) => { if (!o) setBatchDetail(null); }}>
        <SheetContent side="right" className="!w-[75vw] !max-w-[75vw] overflow-y-auto flex flex-col gap-4 p-6">
          {batchLoading && (
            <div className="py-12 text-center text-sm text-muted-foreground">Loading…</div>
          )}
          {batchDetail && <TxnBatchDetailPanel batch={batchDetail} onClose={() => setBatchDetail(null)} />}
        </SheetContent>
      </Sheet>

      <AlertDialog open={!!deleteTarget} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete transaction?</AlertDialogTitle>
            <AlertDialogDescription>
              {deleteTarget?.txn_type} of {deleteTarget && formatQty(deleteTarget.quantity)} units of{" "}
              <strong>{deleteTarget?.instrument_name}</strong> on {deleteTarget && formatDate(deleteTarget.trade_date)}.
              This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function SortHeader({ colId, label, right, sortCol, sortDir, onSort }: {
  colId: SortCol;
  label: string;
  right?: boolean;
  sortCol: SortCol;
  sortDir: SortDir;
  onSort: (col: SortCol) => void;
}) {
  const active = sortCol === colId;
  return (
    <button
      className={cn("flex items-center gap-1 text-xs font-medium hover:text-foreground transition-colors", right && "ml-auto")}
      onClick={() => onSort(colId)}
    >
      {label}
      {active
        ? sortDir === "asc" ? <ArrowUp className="size-3" /> : <ArrowDown className="size-3" />
        : <ArrowUpDown className="size-3 opacity-40" />
      }
    </button>
  );
}

function TxnBatchDetailPanel({ batch, onClose }: { batch: ImportBatch; onClose: () => void }) {
  const filePaths: string[] = (() => {
    if (!batch.file_name) return [];
    try { return JSON.parse(batch.file_name) as string[]; } catch { return []; }
  })();

  const hasCharges = batch.stt_paise + batch.stamp_charges_paise + batch.gst_paise +
    batch.trans_charges_paise + batch.other_charges_paise + batch.total_payable_paise > 0;

  return (
    <>
      <SheetHeader className="pb-2">
        <div className="flex items-center gap-2 mb-1">
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-muted transition-colors text-muted-foreground"
          >
            <ChevronLeft className="size-4" />
          </button>
          <SheetTitle className="font-mono text-sm">{batch.ref_no ?? "Import Batch"}</SheetTitle>
        </div>
        <div className="flex items-start justify-between gap-3">
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            {batch.broker && <span>{batch.broker}</span>}
            {batch.batch_trade_date && <span>{formatDate(batch.batch_trade_date)}</span>}
            <span>Imported {new Date(batch.imported_at).toLocaleString("en-IN", { dateStyle: "medium", timeStyle: "short" })}</span>
            <span>{batch.record_count} records</span>
          </div>
        </div>
        {filePaths.length > 0 && (
          <div className="flex flex-col gap-1 mt-1">
            {filePaths.map((fp, i) => (
              <button
                key={i}
                className="text-xs text-blue-600 dark:text-blue-400 hover:underline text-left font-mono truncate"
                onClick={() => openPath(fp).catch(() => {})}
                title={fp}
              >
                {fp.split(/[\\/]/).pop()}
              </button>
            ))}
          </div>
        )}
      </SheetHeader>

      {hasCharges && (
        <div className="rounded-md border p-3 grid grid-cols-3 gap-x-6 gap-y-1 text-xs">
          {[
            ["STT", batch.stt_paise],
            ["Stamp Duty", batch.stamp_charges_paise],
            ["GST", batch.gst_paise],
            ["Transaction Charges", batch.trans_charges_paise],
            ["Other Charges", batch.other_charges_paise],
            ["Net Payable", batch.total_payable_paise],
          ].filter(([, v]) => (v as number) !== 0).map(([label, paise]) => (
            <div key={label as string} className="flex justify-between col-span-1">
              <span className="text-muted-foreground">{label}</span>
              <span className="font-mono tabular-nums">{formatINR(paise as number)}</span>
            </div>
          ))}
        </div>
      )}

      <div className="flex-1 overflow-auto">
        <table className="w-full text-xs">
          <thead className="bg-muted sticky top-0">
            <tr>
              {["Date", "Instrument", "Type", "Qty", "Price", "Total"].map(h => (
                <th key={h} className="px-2 py-1.5 text-left font-medium text-muted-foreground">{h}</th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y">
            {batch.transactions.map(t => (
              <tr key={t.txn_id} className="hover:bg-muted/30">
                <td className="px-2 py-1.5 tabular-nums">{formatDate(t.trade_date)}</td>
                <td className="px-2 py-1.5 max-w-[160px] truncate">{t.instrument_name}</td>
                <td className="px-2 py-1.5">{t.txn_type}</td>
                <td className="px-2 py-1.5 tabular-nums text-right">{formatQty(t.quantity)}</td>
                <td className="px-2 py-1.5 tabular-nums text-right">{formatINR(t.price_paise)}</td>
                <td className={cn("px-2 py-1.5 tabular-nums text-right font-medium",
                  t.total_value_paise > 0 ? "text-green-600 dark:text-green-400"
                  : t.total_value_paise < 0 ? "text-red-600 dark:text-red-400" : ""
                )}>
                  {t.total_value_paise < 0 ? "−" : "+"}₹{Math.abs(t.total_value_paise / 100).toLocaleString("en-IN", { minimumFractionDigits: 2 })}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
