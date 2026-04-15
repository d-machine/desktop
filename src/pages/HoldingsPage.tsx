import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useReactTable, getCoreRowModel, getSortedRowModel,
  getFilteredRowModel, flexRender,
  type ColumnDef, type SortingState,
} from "@tanstack/react-table";
import { ArrowUpDown, ArrowUp, ArrowDown, TrendingUp, TrendingDown } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { formatINR, formatQty, formatDate } from "@/lib/format";
import { cn } from "@/lib/utils";

interface Holding {
  instrument_id: number;
  instrument_name: string;
  isin?: string;
  asset_class: string;
  account_id: number;
  account_name: string;
  portfolio_id: number;
  quantity: number;
  avg_cost_paise: number;
  total_cost_paise: number;
  current_price_paise?: number;
  current_value_paise?: number;
  unrealized_pnl_paise?: number;
  unrealized_pnl_pct?: number;
  price_date?: string;
}

interface Portfolio { portfolio_id: number; name: string; }
interface Account { account_id: number; portfolio_id: number; name: string; account_type: string; broker?: string; }

interface PortfolioSummary {
  total_invested_paise: number;
  current_value_paise?: number;
  unrealized_pnl_paise?: number;
  unrealized_pnl_pct?: number;
  holdings_count: number;
  accounts_count: number;
}

const ASSET_CLASS_COLORS: Record<string, string> = {
  EQUITY:       "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400",
  MF:           "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400",
  FIXED_INCOME: "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
  DERIVATIVE:   "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400",
  COMMODITY:    "bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400",
};

type AssetFilter = "ALL" | "EQUITY" | "MF" | "FIXED_INCOME" | "DERIVATIVE" | "COMMODITY";

export function HoldingsPage() {
  const [holdings, setHoldings] = useState<Holding[]>([]);
  const [summary, setSummary] = useState<PortfolioSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [assetFilter, setAssetFilter] = useState<AssetFilter>("ALL");
  const [sorting, setSorting] = useState<SortingState>([{ id: "total_cost_paise", desc: true }]);

  const [portfolios, setPortfolios] = useState<Portfolio[]>([]);
  const [accounts, setAccounts]     = useState<Account[]>([]);
  const [filterPortfolio, setFilterPortfolio] = useState<string>("all");
  const [filterAccount, setFilterAccount]     = useState<string>("all");

  // Accounts shown in the account dropdown (restricted to selected portfolio)
  const visibleAccounts = useMemo(() =>
    filterPortfolio === "all"
      ? accounts
      : accounts.filter(a => a.portfolio_id.toString() === filterPortfolio),
    [accounts, filterPortfolio]
  );

  // account_ids to pass to backend (null = all)
  const activeAccountIds = useMemo((): number[] | null => {
    if (filterAccount !== "all") return [parseInt(filterAccount)];
    if (filterPortfolio !== "all") return visibleAccounts.map(a => a.account_id);
    return null;
  }, [filterAccount, filterPortfolio, visibleAccounts]);

  const load = async (ids: number[] | null = activeAccountIds) => {
    setLoading(true);
    try {
      const [h, s] = await Promise.all([
        invoke<Holding[]>("get_holdings", { accountIds: ids }),
        invoke<PortfolioSummary>("get_portfolio_summary", { accountIds: ids }),
      ]);
      setHoldings(h);
      setSummary(s);
    } finally {
      setLoading(false);
    }
  };

  // Initial load: portfolios + accounts + holdings
  useEffect(() => {
    Promise.all([
      invoke<Portfolio[]>("get_portfolios"),
      invoke<Account[]>("get_accounts", { portfolioId: null }),
    ]).then(([ps, as_]) => {
      setPortfolios(ps);
      setAccounts(as_);
    });
    load(null);
  }, []);

  // Reload holdings when filter changes
  useEffect(() => { load(); }, [filterPortfolio, filterAccount]);

  const filtered = useMemo(() =>
    assetFilter === "ALL" ? holdings : holdings.filter(h => h.asset_class === assetFilter),
    [holdings, assetFilter]
  );

  const assetClasses = useMemo(() =>
    ["ALL", ...Array.from(new Set(holdings.map(h => h.asset_class)))],
    [holdings]
  );

  const columns = useMemo<ColumnDef<Holding>[]>(() => [
    {
      id: "instrument_name",
      accessorKey: "instrument_name",
      header: ({ column }) => <SortHeader column={column} label="Instrument" />,
      cell: ({ row }) => (
        <div>
          <div className="text-sm font-medium">{row.original.instrument_name}</div>
          <div className="flex items-center gap-1.5 mt-0.5">
            {row.original.isin && (
              <span className="text-xs text-muted-foreground font-mono">{row.original.isin}</span>
            )}
            <span className={cn("text-xs px-1.5 py-0 rounded", ASSET_CLASS_COLORS[row.original.asset_class])}>
              {row.original.asset_class}
            </span>
          </div>
        </div>
      ),
    },
    {
      id: "account_name",
      accessorKey: "account_name",
      header: "Account",
      cell: ({ row }) => <span className="text-sm text-muted-foreground">{row.original.account_name}</span>,
      size: 140,
    },
    {
      id: "quantity",
      accessorKey: "quantity",
      header: ({ column }) => <SortHeader column={column} label="Quantity" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block">{formatQty(row.original.quantity)}</span>
      ),
      size: 90,
    },
    {
      id: "avg_cost_paise",
      accessorKey: "avg_cost_paise",
      header: ({ column }) => <SortHeader column={column} label="Avg Cost" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block">{formatINR(row.original.avg_cost_paise)}</span>
      ),
      size: 110,
    },
    {
      id: "total_cost_paise",
      accessorKey: "total_cost_paise",
      header: ({ column }) => <SortHeader column={column} label="Invested" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block font-medium">{formatINR(row.original.total_cost_paise)}</span>
      ),
      size: 120,
    },
    {
      id: "current_price_paise",
      accessorKey: "current_price_paise",
      header: ({ column }) => <SortHeader column={column} label="LTP" right />,
      cell: ({ row }) => row.original.current_price_paise != null ? (
        <div className="text-right">
          <div className="text-sm tabular-nums">{formatINR(row.original.current_price_paise)}</div>
          {row.original.price_date && (
            <div className="text-xs text-muted-foreground">{formatDate(row.original.price_date)}</div>
          )}
        </div>
      ) : <span className="text-xs text-muted-foreground block text-right">—</span>,
      size: 110,
    },
    {
      id: "current_value_paise",
      accessorKey: "current_value_paise",
      header: ({ column }) => <SortHeader column={column} label="Market Value" right />,
      cell: ({ row }) => row.original.current_value_paise != null ? (
        <span className="text-sm tabular-nums text-right block font-medium">
          {formatINR(row.original.current_value_paise)}
        </span>
      ) : <span className="text-xs text-muted-foreground block text-right">—</span>,
      size: 120,
    },
    {
      id: "unrealized_pnl_paise",
      accessorKey: "unrealized_pnl_paise",
      header: ({ column }) => <SortHeader column={column} label="P&L" right />,
      cell: ({ row }) => {
        const pnl = row.original.unrealized_pnl_paise;
        const pct = row.original.unrealized_pnl_pct;
        if (pnl == null) return <span className="text-xs text-muted-foreground block text-right">—</span>;
        const positive = pnl >= 0;
        return (
          <div className={cn("text-right", positive ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400")}>
            <div className="text-sm tabular-nums font-medium flex items-center justify-end gap-0.5">
              {positive ? <TrendingUp className="size-3.5" /> : <TrendingDown className="size-3.5" />}
              {positive ? "+" : ""}{formatINR(pnl)}
            </div>
            {pct != null && (
              <div className="text-xs">{positive ? "+" : ""}{pct.toFixed(2)}%</div>
            )}
          </div>
        );
      },
      size: 130,
    },
  ], []);

  const table = useReactTable({
    data: filtered,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
  });

  const pnlPositive = (summary?.unrealized_pnl_paise ?? 0) >= 0;

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-semibold">Holdings</h1>
        <p className="text-sm text-muted-foreground">
          {summary?.holdings_count ?? 0} position{summary?.holdings_count !== 1 ? "s" : ""} across {summary?.accounts_count ?? 0} account{summary?.accounts_count !== 1 ? "s" : ""}
        </p>
      </div>

      {/* Summary cards */}
      {summary && (
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
          <SummaryCard
            label="Total Invested"
            value={formatINR(summary.total_invested_paise)}
          />
          <SummaryCard
            label="Current Value"
            value={summary.current_value_paise != null ? formatINR(summary.current_value_paise) : "—"}
            note={summary.current_value_paise == null ? "Sync prices to see" : undefined}
          />
          <SummaryCard
            label="Unrealized P&L"
            value={summary.unrealized_pnl_paise != null
              ? `${pnlPositive ? "+" : ""}${formatINR(summary.unrealized_pnl_paise)}`
              : "—"
            }
            valueClass={summary.unrealized_pnl_paise != null
              ? pnlPositive ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"
              : ""
            }
          />
          <SummaryCard
            label="Overall Return"
            value={summary.unrealized_pnl_pct != null
              ? `${pnlPositive ? "+" : ""}${summary.unrealized_pnl_pct.toFixed(2)}%`
              : "—"
            }
            valueClass={summary.unrealized_pnl_pct != null
              ? pnlPositive ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"
              : ""
            }
          />
        </div>
      )}

      {/* Portfolio / Account filter */}
      <div className="flex gap-2 flex-wrap">
        <Select value={filterPortfolio} onValueChange={(v) => { setFilterPortfolio(v ?? "all"); setFilterAccount("all"); }}>
          <SelectTrigger className="w-44 h-8 text-xs">
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
          <SelectTrigger className="w-44 h-8 text-xs">
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

      {/* Asset class filter tabs */}
      {assetClasses.length > 1 && (
        <div className="flex gap-1.5 flex-wrap">
          {assetClasses.map((cls) => (
            <button
              key={cls}
              onClick={() => setAssetFilter(cls as AssetFilter)}
              className={cn(
                "px-3 py-1 rounded-full text-xs font-medium border transition-colors",
                assetFilter === cls
                  ? "bg-primary text-primary-foreground border-primary"
                  : "bg-background text-muted-foreground border-border hover:border-foreground/30"
              )}
            >
              {cls === "ALL" ? "All" : cls}
              {cls !== "ALL" && (
                <span className="ml-1 opacity-60">
                  ({holdings.filter(h => h.asset_class === cls).length})
                </span>
              )}
            </button>
          ))}
        </div>
      )}

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
                    <p className="text-muted-foreground text-sm">No holdings yet.</p>
                    <p className="text-xs text-muted-foreground mt-1">Add transactions to see your portfolio here.</p>
                  </td>
                </tr>
              ) : (
                table.getRowModel().rows.map((row) => (
                  <tr key={row.id} className="hover:bg-muted/30 transition-colors">
                    {row.getVisibleCells().map((cell) => (
                      <td key={cell.id} className="px-3 py-2.5">
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
    </div>
  );
}

function SummaryCard({ label, value, note, valueClass }: {
  label: string; value: string; note?: string; valueClass?: string;
}) {
  return (
    <Card>
      <CardContent className="pt-4 pb-4">
        <p className="text-xs text-muted-foreground">{label}</p>
        <p className={cn("text-xl font-semibold mt-0.5 tabular-nums", valueClass)}>{value}</p>
        {note && <p className="text-xs text-muted-foreground mt-0.5">{note}</p>}
      </CardContent>
    </Card>
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
