import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useReactTable, getCoreRowModel, getSortedRowModel,
  getFilteredRowModel, flexRender,
  type ColumnDef, type SortingState,
} from "@tanstack/react-table";
import { Plus, Upload, Trash2, ArrowUpDown, ArrowUp, ArrowDown, Pencil, AlertTriangle, CheckCircle2, X, FileText, ChevronLeft } from "lucide-react";
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

export function TransactionsPage() {
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [accounts, setAccounts]         = useState<Account[]>([]);
  const [portfolios, setPortfolios]     = useState<Portfolio[]>([]);
  const [globalFilter, setGlobalFilter] = useState("");
  const [sorting, setSorting] = useState<SortingState>([{ id: "trade_date", desc: true }]);
  const [showAdd, setShowAdd]       = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Transaction | null>(null);
  const [editTarget, setEditTarget]     = useState<Transaction | null>(null);
  const [loading, setLoading] = useState(true);
  const [batchDetail, setBatchDetail]   = useState<ImportBatch | null>(null);
  const [batchLoading, setBatchLoading] = useState(false);

  const [filterPortfolio, setFilterPortfolio] = useState<string>("all");
  const [filterAccount, setFilterAccount]     = useState<string>("all");
  const [flagFilter, setFlagFilter]           = useState<FlagFilter>("all");

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

  const load = async (ids?: number[]) => {
    setLoading(true);
    try {
      const txns = await invoke<Transaction[]>("get_transactions", {
        filter: {
          ...(ids ? { account_ids: ids } : {}),
          ...(flagFilter !== "all" ? { flag_filter: flagFilter } : {}),
        },
      });
      setTransactions(txns);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    Promise.all([
      invoke<Portfolio[]>("get_portfolios"),
      invoke<Account[]>("get_accounts", { portfolioId: null }),
    ]).then(([ps, as_]) => {
      setPortfolios(ps);
      setAccounts(as_);
    });
    load();
  }, []);

  useEffect(() => { load(activeAccountIds); }, [filterPortfolio, filterAccount, flagFilter]);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    await invoke("delete_transaction", { txnId: deleteTarget.txn_id });
    setDeleteTarget(null);
    await load(activeAccountIds);
  };

  const handleDismissFlag = async (txn: Transaction) => {
    await invoke("dismiss_transaction_flag", { txnId: txn.txn_id });
    await load(activeAccountIds);
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

  const flaggedCount = useMemo(
    () => transactions.filter(t => t.flag && !t.flag_dismissed).length,
    [transactions]
  );

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
      accessorKey: "trade_date",
      header: ({ column }) => <SortHeader column={column} label="Date" />,
      cell: ({ row }) => (
        <span className={cn("text-sm tabular-nums", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {formatDate(row.original.trade_date)}
        </span>
      ),
      size: 100,
    },
    {
      id: "instrument_name",
      accessorKey: "instrument_name",
      header: ({ column }) => <SortHeader column={column} label="Instrument" />,
      cell: ({ row }) => (
        <div className={cn(row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          <div className="text-sm font-medium truncate max-w-[200px]">{row.original.instrument_name}</div>
          {row.original.isin && <div className="text-xs text-muted-foreground font-mono">{row.original.isin}</div>}
        </div>
      ),
    },
    {
      id: "txn_type",
      accessorKey: "txn_type",
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
      accessorKey: "account_name",
      header: "Account",
      cell: ({ row }) => (
        <span className={cn("text-sm text-muted-foreground", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {row.original.account_name}
        </span>
      ),
    },
    {
      id: "quantity",
      accessorKey: "quantity",
      header: ({ column }) => <SortHeader column={column} label="Qty" right />,
      cell: ({ row }) => (
        <span className={cn("text-sm tabular-nums text-right block", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {formatQty(row.original.quantity)}
        </span>
      ),
      size: 80,
    },
    {
      id: "price_paise",
      accessorKey: "price_paise",
      header: ({ column }) => <SortHeader column={column} label="Price" right />,
      cell: ({ row }) => (
        <span className={cn("text-sm tabular-nums text-right block", row.original.flag && !row.original.flag_dismissed && "opacity-50")}>
          {formatINR(row.original.price_paise)}
        </span>
      ),
      size: 110,
    },
    {
      id: "total_value_paise",
      accessorKey: "total_value_paise",
      header: ({ column }) => <SortHeader column={column} label="Total" right />,
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
                  <TooltipTrigger >
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
  ], [transactions]);

  const table = useReactTable({
    data: transactions,
    columns,
    state: { globalFilter, sorting },
    onGlobalFilterChange: setGlobalFilter,
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    globalFilterFn: "includesString",
  });

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Transactions</h1>
          <p className="text-sm text-muted-foreground">
            {transactions.length.toLocaleString()} transaction{transactions.length !== 1 ? "s" : ""}
            {flaggedCount > 0 && (
              <span className="ml-2 text-amber-600 font-medium">
                · {flaggedCount} issue{flaggedCount !== 1 ? "s" : ""}
              </span>
            )}
          </p>
        </div>
        <div className="flex gap-2">
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
          value={globalFilter}
          onChange={(e) => setGlobalFilter(e.target.value)}
          className="w-64 h-8 text-xs"
        />
        <Select value={filterPortfolio} onValueChange={(v) => { setFilterPortfolio(v ?? "all"); setFilterAccount("all"); }}>
          <SelectTrigger className="w-40 h-8 text-xs">
            <SelectValue placeholder="All Portfolios" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All Portfolios</SelectItem>
            {portfolios.map(p => (
              <SelectItem key={p.portfolio_id} value={p.portfolio_id.toString()}>{p.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={filterAccount} onValueChange={(v) => setFilterAccount(v ?? "all")} disabled={visibleAccounts.length === 0}>
          <SelectTrigger className="w-40 h-8 text-xs">
            <SelectValue placeholder="All Accounts" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All Accounts</SelectItem>
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
              onClick={() => setFlagFilter(f)}
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
                  <td colSpan={columns.length} className="px-3 py-12 text-center">
                    <p className="text-muted-foreground text-sm">
                      {flagFilter === "flagged" ? "No flagged transactions." : "No transactions yet."}
                    </p>
                    {flagFilter === "all" && (
                      <p className="text-xs text-muted-foreground mt-1">Add one manually or import from a broker statement.</p>
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

      {/* Dialogs */}
      <AddTransactionDialog
        open={showAdd}
        onOpenChange={setShowAdd}
        accounts={accounts}
        onSaved={() => {
          load(activeAccountIds);
          invoke("resolve_instruments")
            .catch(() => {})
            .then(() => invoke("sync_prices", { force: true }).catch(() => {}));
        }}
      />
      <EditTransactionDialog
        open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)}
        transaction={editTarget}
        onSaved={() => load(activeAccountIds)}
      />
      <ImportDialog
        open={showImport}
        onOpenChange={setShowImport}
        accounts={accounts}
        onImported={() => {
          invoke<Account[]>("get_accounts", { portfolioId: null }).then(setAccounts);
          load(activeAccountIds);
          // Resolve new instruments against server catalog (fills ISINs, merges duplicates),
          // then sync prices for anything that got resolved.
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
            <span className="opacity-60">{batch.source_type}</span>
            <span>Imported {new Date(batch.imported_at + "Z").toLocaleDateString()}</span>
          </div>
          {filePaths.length > 0 && (
            <div className="flex flex-col gap-1 items-end shrink-0">
              {filePaths.map((fp, i) => (
                <button
                  key={i}
                  onClick={() => openPath(fp).catch(() => {})}
                  className="flex items-center gap-1 text-xs text-primary hover:underline"
                  title={fp}
                >
                  <FileText className="size-3" />
                  <span className="max-w-[200px] truncate">{fp.split("/").pop()}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </SheetHeader>

      {hasCharges && (
        <div className="rounded-lg border bg-muted/20 overflow-hidden">
          <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide px-4 pt-3 pb-2">Charges</p>
          <table className="w-full text-sm">
            <tbody className="divide-y">
              {batch.stt_paise > 0 && <tr><td className="py-2 px-4 text-muted-foreground">STT</td><td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.stt_paise)}</td></tr>}
              {batch.stamp_charges_paise > 0 && <tr><td className="py-2 px-4 text-muted-foreground">Stamp Duty</td><td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.stamp_charges_paise)}</td></tr>}
              {batch.gst_paise > 0 && <tr><td className="py-2 px-4 text-muted-foreground">GST</td><td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.gst_paise)}</td></tr>}
              {batch.trans_charges_paise > 0 && <tr><td className="py-2 px-4 text-muted-foreground">Transaction Charges</td><td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.trans_charges_paise)}</td></tr>}
              {batch.other_charges_paise > 0 && <tr><td className="py-2 px-4 text-muted-foreground">Other Charges</td><td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.other_charges_paise)}</td></tr>}
              {batch.total_payable_paise > 0 && <tr className="bg-muted/30 font-medium"><td className="py-2.5 px-4">Net Payable</td><td className="py-2.5 px-4 text-right tabular-nums font-semibold">{formatINR(batch.total_payable_paise)}</td></tr>}
            </tbody>
          </table>
        </div>
      )}

      <div>
        <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">
          Trades · {batch.transactions.length}
        </p>
        <div className="rounded-lg border overflow-hidden">
          <table className="w-full text-xs">
            <thead>
              <tr className="border-b bg-muted/30 text-muted-foreground">
                <th className="py-2 px-3 text-left font-medium">Security</th>
                <th className="py-2 px-3 text-left font-medium">Side</th>
                <th className="py-2 px-3 text-right font-medium">Qty</th>
                <th className="py-2 px-3 text-right font-medium">Price</th>
                <th className="py-2 px-3 text-right font-medium">Brokerage</th>
                <th className="py-2 px-3 text-right font-medium">Net Value</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {batch.transactions.map((t) => {
                const isBuy = ["BUY","SIP","OPENING_BALANCE","BONUS","MERGER_IN","SWITCH_IN","TRANSFER_IN"].includes(t.txn_type);
                const color = TXN_TYPE_COLORS[t.txn_type] ?? "";
                return (
                  <tr key={t.txn_id} className="hover:bg-muted/20">
                    <td className="py-2 px-3 font-medium truncate max-w-[180px]">{t.instrument_name}</td>
                    <td className={cn("py-2 px-3 font-semibold", color)}>{t.txn_type}</td>
                    <td className="py-2 px-3 text-right tabular-nums">{formatQty(t.quantity)}</td>
                    <td className="py-2 px-3 text-right tabular-nums">{formatINR(t.price_paise)}</td>
                    <td className="py-2 px-3 text-right tabular-nums text-muted-foreground">
                      {t.brokerage_paise > 0 ? formatINR(t.brokerage_paise) : "—"}
                    </td>
                    <td className={cn("py-2 px-3 text-right tabular-nums font-medium", isBuy ? "text-red-600 dark:text-red-400" : "text-green-600 dark:text-green-400")}>
                      {isBuy ? "−" : "+"}{formatINR(Math.abs(t.total_value_paise))}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}

function SortHeader({ column, label, right }: { column: any; label: string; right?: boolean }) {
  const sorted = column.getIsSorted();
  return (
    <button
      className={cn("flex items-center gap-1 hover:text-foreground transition-colors", right && "ml-auto")}
      onClick={() => column.toggleSorting()}
    >
      {label}
      {sorted === "asc" ? <ArrowUp className="size-3" /> :
       sorted === "desc" ? <ArrowDown className="size-3" /> :
       <ArrowUpDown className="size-3 opacity-40" />}
    </button>
  );
}
