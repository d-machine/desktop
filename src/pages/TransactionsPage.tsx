import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useReactTable, getCoreRowModel, getSortedRowModel,
  getFilteredRowModel, flexRender,
  type ColumnDef, type SortingState,
} from "@tanstack/react-table";
import { Plus, Upload, Trash2, ArrowUpDown, ArrowUp, ArrowDown } from "lucide-react";
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
import { AddTransactionDialog } from "@/components/transactions/AddTransactionDialog";
import { ImportDialog } from "@/components/transactions/ImportDialog";
import { formatINR, formatQty, formatDate } from "@/lib/format";
import { TXN_TYPE_COLORS } from "@/lib/txn-types";
import { cn } from "@/lib/utils";

interface Transaction {
  txn_id: number;
  account_id: number;
  account_name: string;
  instrument_name: string;
  isin?: string;
  txn_type: string;
  trade_segment: string;
  trade_date: string;
  quantity: number;
  price_paise: number;
  total_value_paise: number;
  brokerage_paise: number;
  stt_paise: number;
  other_charges_paise: number;
  notes?: string;
}

interface Portfolio { portfolio_id: number; name: string; }
interface Account { account_id: number; portfolio_id: number; name: string; account_type: string; broker?: string; }

export function TransactionsPage() {
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [accounts, setAccounts]         = useState<Account[]>([]);
  const [portfolios, setPortfolios]     = useState<Portfolio[]>([]);
  const [globalFilter, setGlobalFilter] = useState("");
  const [sorting, setSorting] = useState<SortingState>([{ id: "trade_date", desc: true }]);
  const [showAdd, setShowAdd]       = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Transaction | null>(null);
  const [loading, setLoading] = useState(true);

  const [filterPortfolio, setFilterPortfolio] = useState<string>("all");
  const [filterAccount, setFilterAccount]     = useState<string>("all");

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
        filter: ids ? { account_ids: ids } : {},
      });
      setTransactions(txns);
    } finally {
      setLoading(false);
    }
  };

  // Initial load
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

  // Reload when filter changes
  useEffect(() => { load(activeAccountIds); }, [filterPortfolio, filterAccount]);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    await invoke("delete_transaction", { txnId: deleteTarget.txn_id });
    setDeleteTarget(null);
    await load();
  };

  const columns = useMemo<ColumnDef<Transaction>[]>(() => [
    {
      id: "trade_date",
      accessorKey: "trade_date",
      header: ({ column }) => <SortHeader column={column} label="Date" />,
      cell: ({ row }) => <span className="text-sm tabular-nums">{formatDate(row.original.trade_date)}</span>,
      size: 100,
    },
    {
      id: "instrument_name",
      accessorKey: "instrument_name",
      header: ({ column }) => <SortHeader column={column} label="Instrument" />,
      cell: ({ row }) => (
        <div>
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
        <div className="flex flex-col gap-0.5">
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
      cell: ({ row }) => <span className="text-sm text-muted-foreground">{row.original.account_name}</span>,
    },
    {
      id: "quantity",
      accessorKey: "quantity",
      header: ({ column }) => <SortHeader column={column} label="Qty" right />,
      cell: ({ row }) => <span className="text-sm tabular-nums text-right block">{formatQty(row.original.quantity)}</span>,
      size: 80,
    },
    {
      id: "price_paise",
      accessorKey: "price_paise",
      header: ({ column }) => <SortHeader column={column} label="Price" right />,
      cell: ({ row }) => <span className="text-sm tabular-nums text-right block">{formatINR(row.original.price_paise)}</span>,
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
            val > 0 ? "text-green-600 dark:text-green-400" : val < 0 ? "text-red-600 dark:text-red-400" : ""
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
      cell: ({ row }) => (
        <button
          className="p-1 text-muted-foreground hover:text-destructive transition-colors opacity-0 group-hover:opacity-100"
          onClick={() => setDeleteTarget(row.original)}
        >
          <Trash2 className="size-3.5" />
        </button>
      ),
      size: 36,
    },
  ], []);

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
      <div className="flex gap-2 flex-wrap">
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
      </div>

      {/* Table */}
      <div className="flex-1 border rounded-lg overflow-hidden">
        <div className="overflow-auto h-full">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 sticky top-0 z-10">
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
                    <p className="text-muted-foreground text-sm">No transactions yet.</p>
                    <p className="text-xs text-muted-foreground mt-1">Add one manually or import from a broker statement.</p>
                  </td>
                </tr>
              ) : (
                table.getRowModel().rows.map((row) => (
                  <tr key={row.id} className="group hover:bg-muted/30 transition-colors">
                    {row.getVisibleCells().map((cell) => (
                      <td key={cell.id} className="px-3 py-2">
                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                      </td>
                    ))}
                  </tr>
                ))
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
        onSaved={load}
      />
      <ImportDialog
        open={showImport}
        onOpenChange={setShowImport}
        accounts={accounts}
        onImported={load}
      />
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
