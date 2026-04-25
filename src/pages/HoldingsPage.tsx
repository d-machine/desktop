import { useEffect, useState, useMemo, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  TrendingUp, TrendingDown, ArrowRightLeft, RefreshCw,
  ChevronRight, ChevronDown, Check, ChevronsUpDown, X, FileText,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Popover, PopoverContent, PopoverTrigger,
} from "@/components/ui/popover";
import {
  Sheet, SheetContent, SheetHeader, SheetTitle,
} from "@/components/ui/sheet";
import { formatINR, formatQty, formatDate } from "@/lib/format";
import { cn } from "@/lib/utils";
import { TransferDialog } from "@/components/holdings/TransferDialog";

// ─── Types ───────────────────────────────────────────────────────────────────

interface Holding {
  instrument_id: number;
  instrument_name: string;
  isin?: string;
  instrument_type: string;
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

interface Transaction {
  txn_id: number;
  txn_type: string;
  trade_date: string;
  trade_segment: string;
  quantity: number;
  price_paise: number;
  brokerage_paise: number;
  stt_paise: number;
  other_charges_paise: number;
  total_value_paise: number;
  notes?: string;
  broker_ref?: string;
  flag?: string;
  flag_reason?: string;
  flag_dismissed: boolean;
  batch_id?: number;
  instrument_name?: string;
}

interface ImportBatch {
  batch_id: number;
  account_id: number;
  source_type: string;
  file_name?: string;   // JSON array of original file paths, e.g. ["/path/to/statement.pdf"]
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

// ─── Constants ───────────────────────────────────────────────────────────────

const GROUP_ORDER = [
  "EQUITY", "MF", "FIXED_INCOME", "INSURANCE",
  "DERIVATIVE", "COMMODITY", "REAL_ESTATE", "ALTERNATIVES",
];

const GROUP_LABELS: Record<string, string> = {
  EQUITY:       "Equity",
  MF:           "Mutual Funds",
  FIXED_INCOME: "Fixed Income",
  INSURANCE:    "Insurance",
  DERIVATIVE:   "Derivatives",
  COMMODITY:    "Commodities",
  REAL_ESTATE:  "Real Estate",
  ALTERNATIVES: "Alternatives",
};

const ASSET_CLASS_COLORS: Record<string, string> = {
  EQUITY:       "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400",
  MF:           "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400",
  FIXED_INCOME: "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
  INSURANCE:    "bg-teal-100 text-teal-700 dark:bg-teal-900/30 dark:text-teal-400",
  DERIVATIVE:   "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400",
  COMMODITY:    "bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400",
  REAL_ESTATE:  "bg-rose-100 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400",
  ALTERNATIVES: "bg-slate-100 text-slate-700 dark:bg-slate-900/30 dark:text-slate-400",
};

const TXN_TYPE_COLORS: Record<string, string> = {
  BUY:             "text-blue-600 dark:text-blue-400",
  SIP:             "text-blue-600 dark:text-blue-400",
  OPENING_BALANCE: "text-slate-500",
  BONUS:           "text-green-600 dark:text-green-400",
  MERGER_IN:       "text-purple-600 dark:text-purple-400",
  SWITCH_IN:       "text-purple-600 dark:text-purple-400",
  TRANSFER_IN:     "text-teal-600 dark:text-teal-400",
  SELL:            "text-red-600 dark:text-red-400",
  REDEMPTION:      "text-red-600 dark:text-red-400",
  MERGER_OUT:      "text-orange-600 dark:text-orange-400",
  SWITCH_OUT:      "text-orange-600 dark:text-orange-400",
  TRANSFER_OUT:    "text-orange-600 dark:text-orange-400",
  DIVIDEND:        "text-green-600 dark:text-green-400",
  INTEREST:        "text-green-600 dark:text-green-400",
};

// ─── Main Page ───────────────────────────────────────────────────────────────

export function HoldingsPage({ initialInstrumentId }: { initialInstrumentId?: number } = {}) {
  const [holdings, setHoldings]       = useState<Holding[]>([]);
  const [summary, setSummary]         = useState<PortfolioSummary | null>(null);
  const [loading, setLoading]         = useState(true);
  const [syncing, setSyncing]         = useState(false);
  const [lastSyncAt, setLastSyncAt]   = useState<string | null>(null);

  const [portfolios, setPortfolios] = useState<Portfolio[]>([]);
  const [accounts, setAccounts]     = useState<Account[]>([]);

  // Multi-select filter state (empty = "all")
  const [selPortfolios, setSelPortfolios] = useState<number[]>([]);
  const [selAccounts, setSelAccounts]     = useState<number[]>([]);
  const [selAssetClasses, setSelAssetClasses] = useState<string[]>([]);

  // UI state
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const [drawerHolding, setDrawerHolding]     = useState<Holding | null>(null);
  const [drawerTxns, setDrawerTxns]           = useState<Transaction[]>([]);
  const [drawerLoading, setDrawerLoading]     = useState(false);
  const [transferHolding, setTransferHolding] = useState<Holding | null>(null);
  const [batchDetail, setBatchDetail]   = useState<ImportBatch | null>(null);
  const [batchLoading, setBatchLoading] = useState(false);
  // "txn" = showing transaction list; "batch" = showing batch detail inside same sheet
  const [drawerView, setDrawerView]     = useState<"txn" | "batch">("txn");

  // Effective account_ids derived from portfolio + account selections
  const effectiveAccountIds = useMemo((): number[] | null => {
    let filtered = accounts;
    if (selPortfolios.length > 0) {
      filtered = filtered.filter(a => selPortfolios.includes(a.portfolio_id));
    }
    if (selAccounts.length > 0) {
      filtered = filtered.filter(a => selAccounts.includes(a.account_id));
    }
    if (filtered.length === accounts.length) return null;
    return filtered.map(a => a.account_id);
  }, [accounts, selPortfolios, selAccounts]);

  // Accounts shown in account filter (scoped to selected portfolios)
  const visibleAccounts = useMemo(() =>
    selPortfolios.length > 0
      ? accounts.filter(a => selPortfolios.includes(a.portfolio_id))
      : accounts,
    [accounts, selPortfolios]
  );

  // All asset_classes present in current holdings
  const presentAssetClasses = useMemo(() =>
    GROUP_ORDER.filter(ac => holdings.some(h => h.asset_class === ac))
      .concat(Array.from(new Set(holdings.map(h => h.asset_class))).filter(ac => !GROUP_ORDER.includes(ac))),
    [holdings]
  );

  const load = useCallback(async (
    accountIds = effectiveAccountIds,
    assetClasses = selAssetClasses,
  ) => {
    setLoading(true);
    try {
      const [h, s] = await Promise.all([
        invoke<Holding[]>("get_holdings", {
          accountIds,
          portfolioIds: null,
          assetClasses: assetClasses.length > 0 ? assetClasses : null,
        }),
        invoke<PortfolioSummary>("get_portfolio_summary", {
          accountIds,
          portfolioIds: null,
          assetClasses: assetClasses.length > 0 ? assetClasses : null,
        }),
      ]);
      setHoldings(h);
      setSummary(s);
    } finally {
      setLoading(false);
    }
  }, [effectiveAccountIds, selAssetClasses]);

  const syncPrices = async () => {
    setSyncing(true);
    try {
      await invoke("resolve_instruments").catch((e: unknown) => console.error("resolve_instruments failed:", e));
      const result = await invoke<{ updated: number; synced_at: string }>("sync_prices", { force: true });
      if (result.synced_at) setLastSyncAt(result.synced_at);
      await load();
    } catch (e) {
      console.error("Price sync failed:", e);
    } finally {
      setSyncing(false);
    }
  };

  // Initial setup
  useEffect(() => {
    Promise.all([
      invoke<Portfolio[]>("get_portfolios"),
      invoke<Account[]>("get_accounts", { portfolioId: null }),
      invoke<string | null>("get_setting", { key: "last_price_sync" }),
    ]).then(([ps, as_, lastSync]) => {
      setPortfolios(ps);
      setAccounts(as_);
      if (lastSync) setLastSyncAt(lastSync);
    });
    load(null, []);
    invoke("resolve_instruments").catch(() => {});
  }, []);

  // Reload on filter change
  useEffect(() => { load(); }, [effectiveAccountIds, selAssetClasses]);

  // Open transaction drawer
  const openDrawer = async (holding: Holding) => {
    setDrawerHolding(holding);
    setDrawerTxns([]);
    setDrawerLoading(true);
    setDrawerView("txn");
    setBatchDetail(null);
    try {
      const txns = await invoke<Transaction[]>("get_transactions", {
        filter: {
          account_ids: [holding.account_id],
          instrument_id: holding.instrument_id,
          limit: 500,
          offset: 0,
        },
      });
      setDrawerTxns(txns);
    } finally {
      setDrawerLoading(false);
    }
  };

  // Auto-open drawer when navigated from dashboard with a pre-selected instrument
  const autoOpenedRef = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (!initialInstrumentId || initialInstrumentId === autoOpenedRef.current || holdings.length === 0) return;
    const h = holdings.find(h => h.instrument_id === initialInstrumentId);
    if (h) {
      autoOpenedRef.current = initialInstrumentId;
      openDrawer(h);
    }
  }, [initialInstrumentId, holdings]);

  const openBatch = async (batchId: number) => {
    setBatchLoading(true);
    setBatchDetail(null);
    setDrawerView("batch");
    try {
      const b = await invoke<ImportBatch>("get_import_batch", { batchId });
      setBatchDetail(b);
    } finally {
      setBatchLoading(false);
    }
  };

  const closeBatch = () => {
    setBatchDetail(null);
    setBatchLoading(false);
    setDrawerView("txn");
  };

  const toggleGroup = (assetClass: string) => {
    setCollapsedGroups(prev => {
      const next = new Set(prev);
      if (next.has(assetClass)) next.delete(assetClass);
      else next.add(assetClass);
      return next;
    });
  };

  const pnlPositive = (summary?.unrealized_pnl_paise ?? 0) >= 0;

  // Group holdings by asset_class in GROUP_ORDER
  const groups = useMemo(() => {
    const map = new Map<string, Holding[]>();
    for (const h of holdings) {
      const arr = map.get(h.asset_class) ?? [];
      arr.push(h);
      map.set(h.asset_class, arr);
    }
    const ordered: [string, Holding[]][] = [];
    for (const ac of GROUP_ORDER) {
      if (map.has(ac)) ordered.push([ac, map.get(ac)!]);
    }
    for (const [ac, hs] of map) {
      if (!GROUP_ORDER.includes(ac)) ordered.push([ac, hs]);
    }
    return ordered.map(([ac, hs]) => ({
      assetClass: ac,
      holdings: [...hs].sort((a, b) => b.total_cost_paise - a.total_cost_paise),
    }));
  }, [holdings]);

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Holdings</h1>
          <p className="text-sm text-muted-foreground">
            {summary?.holdings_count ?? 0} position{summary?.holdings_count !== 1 ? "s" : ""} across {summary?.accounts_count ?? 0} account{summary?.accounts_count !== 1 ? "s" : ""}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {lastSyncAt && (
            <span className="text-xs text-muted-foreground">
              Prices: {new Date(lastSyncAt + "Z").toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
            </span>
          )}
          <Button variant="outline" size="sm" onClick={syncPrices} disabled={syncing} className="h-8 gap-1.5">
            <RefreshCw className={cn("size-3.5", syncing && "animate-spin")} />
            {syncing ? "Syncing…" : "Refresh prices"}
          </Button>
        </div>
      </div>

      {/* Summary cards */}
      {summary && (
        <div className="flex items-center justify-between rounded-lg bg-muted/60 px-5 py-3">
          <SummaryCard label="Total Invested" value={formatINR(summary.total_invested_paise)} />
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

      {/* Filter bar */}
      <div className="flex gap-2 flex-wrap items-center">
        <MultiSelectPopover
          label="Portfolio"
          options={portfolios.map(p => ({ value: p.portfolio_id, label: p.name }))}
          selected={selPortfolios}
          onChange={(ids) => { setSelPortfolios(ids); setSelAccounts([]); }}
        />
        <MultiSelectPopover
          label="Account"
          options={visibleAccounts.map(a => ({ value: a.account_id, label: a.broker ? `${a.name} · ${a.broker}` : a.name }))}
          selected={selAccounts}
          onChange={setSelAccounts}
          disabled={visibleAccounts.length === 0}
        />
        <MultiSelectPopover
          label="Asset Class"
          options={presentAssetClasses.map(ac => ({ value: ac, label: GROUP_LABELS[ac] ?? ac }))}
          selected={selAssetClasses}
          onChange={setSelAssetClasses}
          disabled={presentAssetClasses.length === 0}
        />
        {(selPortfolios.length > 0 || selAccounts.length > 0 || selAssetClasses.length > 0) && (
          <Button
            variant="ghost"
            size="sm"
            className="h-8 text-xs text-muted-foreground gap-1"
            onClick={() => { setSelPortfolios([]); setSelAccounts([]); setSelAssetClasses([]); }}
          >
            <X className="size-3" />
            Clear filters
          </Button>
        )}
      </div>

      {/* Holdings accordion */}
      <div className="flex-1 overflow-auto space-y-2 pb-4">
        {loading ? (
          <div className="flex items-center justify-center py-16 text-muted-foreground text-sm">Loading…</div>
        ) : groups.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 gap-1">
            <p className="text-muted-foreground text-sm">No holdings yet.</p>
            <p className="text-xs text-muted-foreground">Add transactions to see your portfolio here.</p>
          </div>
        ) : (
          groups.map(({ assetClass, holdings: gh }) => (
            <AssetGroup
              key={assetClass}
              assetClass={assetClass}
              holdings={gh}
              collapsed={collapsedGroups.has(assetClass)}
              onToggle={() => toggleGroup(assetClass)}
              onRowClick={openDrawer}
              onTransfer={setTransferHolding}
            />
          ))
        )}
      </div>

      {/* Transaction drawer */}
      <Sheet open={drawerHolding !== null} onOpenChange={(open) => { if (!open) { setDrawerHolding(null); closeBatch(); } }}>
        <SheetContent className="!w-[75vw] !max-w-[75vw] overflow-y-auto flex flex-col gap-0 p-0">
          {drawerHolding && drawerView === "txn" && (
            <TransactionDrawer
              holding={drawerHolding}
              transactions={drawerTxns}
              loading={drawerLoading}
              onOpenBatch={openBatch}
            />
          )}
          {drawerView === "batch" && (
            <BatchView
              batch={batchDetail}
              loading={batchLoading}
              onBack={closeBatch}
            />
          )}
        </SheetContent>
      </Sheet>

      <TransferDialog
        holding={transferHolding}
        accounts={accounts}
        onClose={() => setTransferHolding(null)}
        onDone={() => { setTransferHolding(null); load(); }}
      />
    </div>
  );
}

// ─── AssetGroup ───────────────────────────────────────────────────────────────

function AssetGroup({
  assetClass, holdings, collapsed, onToggle, onRowClick, onTransfer,
}: {
  assetClass: string;
  holdings: Holding[];
  collapsed: boolean;
  onToggle: () => void;
  onRowClick: (h: Holding) => void;
  onTransfer: (h: Holding) => void;
}) {
  const label    = GROUP_LABELS[assetClass] ?? assetClass;
  const color    = ASSET_CLASS_COLORS[assetClass] ?? "bg-slate-100 text-slate-700";
  const invested = holdings.reduce((s, h) => s + h.total_cost_paise, 0);
  const hasValue = holdings.some(h => h.current_value_paise != null);
  const value    = hasValue ? holdings.reduce((s, h) => s + (h.current_value_paise ?? 0), 0) : null;
  const pnl      = hasValue ? holdings.reduce((s, h) => s + (h.unrealized_pnl_paise ?? 0), 0) : null;
  const pnlPos   = (pnl ?? 0) >= 0;

  return (
    <div className="border rounded-lg overflow-hidden">
      {/* Group header */}
      <button
        className="w-full flex items-center gap-3 px-4 py-3 bg-muted/30 hover:bg-muted/50 transition-colors text-left"
        onClick={onToggle}
      >
        {collapsed
          ? <ChevronRight className="size-4 text-muted-foreground shrink-0" />
          : <ChevronDown  className="size-4 text-muted-foreground shrink-0" />
        }
        <span className={cn("text-xs font-medium px-2 py-0.5 rounded shrink-0", color)}>{label}</span>
        <span className="text-xs text-muted-foreground">{holdings.length} holding{holdings.length !== 1 ? "s" : ""}</span>
        <div className="ml-auto flex items-center gap-6 text-right">
          <div className="hidden sm:flex items-center gap-2">
            <p className="text-xs text-muted-foreground leading-none">Invested</p>
            <p className="text-sm font-medium tabular-nums">{formatINR(invested)}</p>
          </div>
          {value != null && (
          <div className="hidden sm:flex items-center gap-2">
              <p className="text-xs text-muted-foreground leading-none">Market Value</p>
              <p className="text-sm font-medium tabular-nums">{formatINR(value)}</p>
            </div>
          )}
          {pnl != null && (
            <div className={cn("flex items-center gap-2", pnlPos ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400")}>
              <p className="text-xs text-muted-foreground leading-none">P&L</p>
              <p className="text-sm font-medium tabular-nums">
                {pnlPos ? "+" : ""}{formatINR(pnl)}
              </p>
            </div>
          )}
        </div>
      </button>

      {/* Holdings rows */}
      {!collapsed && (
        <div className="divide-y">
          <div className="grid grid-cols-[1fr_auto_auto_auto_auto_auto_auto_auto] gap-0 px-4 py-1.5 bg-muted/10 text-xs text-muted-foreground font-medium border-b">
            <span>Instrument</span>
            <span className="text-right w-20">Qty</span>
            <span className="text-right w-24">Avg Cost</span>
            <span className="text-right w-24">Invested</span>
            <span className="text-right w-24">LTP</span>
            <span className="text-right w-28">Mkt Value</span>
            <span className="text-right w-28">P&L</span>
            <span className="w-8" />
          </div>
          {holdings.map((h) => (
            <HoldingRow
              key={`${h.account_id}-${h.instrument_id}`}
              holding={h}
              onClick={() => onRowClick(h)}
              onTransfer={() => onTransfer(h)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ─── HoldingRow ───────────────────────────────────────────────────────────────

function HoldingRow({ holding: h, onClick, onTransfer }: {
  holding: Holding;
  onClick: () => void;
  onTransfer: () => void;
}) {
  const pnl    = h.unrealized_pnl_paise;
  const pct    = h.unrealized_pnl_pct;
  const pnlPos = (pnl ?? 0) >= 0;

  return (
    <div
      className="grid grid-cols-[1fr_auto_auto_auto_auto_auto_auto_auto] gap-0 px-4 py-2.5 hover:bg-muted/20 transition-colors group cursor-pointer items-center"
      onClick={onClick}
    >
      {/* Instrument */}
      <div className="min-w-0 pr-3">
        <p className="text-sm font-medium truncate">{h.instrument_name}</p>
        <div className="flex items-center gap-1.5 mt-0.5 flex-wrap">
          {h.isin && <span className="text-xs text-muted-foreground font-mono">{h.isin}</span>}
          <span className="text-xs text-muted-foreground">{h.account_name}</span>
          <span className={cn("text-xs px-1.5 py-0 rounded leading-5", ASSET_CLASS_COLORS[h.asset_class] ?? "bg-slate-100 text-slate-700")}>
            {h.instrument_type}
          </span>
        </div>
      </div>

      {/* Qty */}
      <span className="text-sm tabular-nums text-right w-20">{formatQty(h.quantity)}</span>

      {/* Avg Cost */}
      <span className="text-sm tabular-nums text-right w-24">{formatINR(h.avg_cost_paise)}</span>

      {/* Invested */}
      <span className="text-sm tabular-nums text-right w-24 font-medium">{formatINR(h.total_cost_paise)}</span>

      {/* LTP */}
      <div className="text-right w-24">
        {h.current_price_paise != null ? (
          <>
            <div className="text-sm tabular-nums">{formatINR(h.current_price_paise)}</div>
            {h.price_date && <div className="text-xs text-muted-foreground">{formatDate(h.price_date)}</div>}
          </>
        ) : (
          <span className="text-xs text-muted-foreground">—</span>
        )}
      </div>

      {/* Mkt Value */}
      <span className="text-sm tabular-nums text-right w-28 font-medium">
        {h.current_value_paise != null ? formatINR(h.current_value_paise) : <span className="text-xs text-muted-foreground">—</span>}
      </span>

      {/* P&L */}
      <div className="text-right w-28">
        {pnl != null ? (
          <div className={cn(pnlPos ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400")}>
            <div className="text-sm tabular-nums font-medium flex items-center justify-end gap-0.5">
              {pnlPos ? <TrendingUp className="size-3" /> : <TrendingDown className="size-3" />}
              {pnlPos ? "+" : ""}{formatINR(pnl)}
            </div>
            {pct != null && <div className="text-xs">{pnlPos ? "+" : ""}{pct.toFixed(2)}%</div>}
          </div>
        ) : (
          <span className="text-xs text-muted-foreground">—</span>
        )}
      </div>

      {/* Actions */}
      <div className="w-8 flex justify-center" onClick={(e) => e.stopPropagation()}>
        <Button
          variant="ghost"
          size="icon"
          className="size-7 opacity-0 group-hover:opacity-100 transition-opacity"
          title="Transfer holding"
          onClick={onTransfer}
        >
          <ArrowRightLeft className="size-3.5" />
        </Button>
      </div>
    </div>
  );
}

// ─── TransactionDrawer ────────────────────────────────────────────────────────

function TransactionDrawer({ holding, transactions, loading, onOpenBatch }: {
  holding: Holding;
  transactions: Transaction[];
  loading: boolean;
  onOpenBatch: (batchId: number) => void;
}) {
  const invested = holding.total_cost_paise;
  const pnl      = holding.unrealized_pnl_paise;
  const pnlPos   = (pnl ?? 0) >= 0;
  const color    = ASSET_CLASS_COLORS[holding.asset_class] ?? "bg-slate-100 text-slate-700";

  return (
    <>
      <SheetHeader className="px-5 pt-5 pb-4 border-b shrink-0">
        <div className="flex items-start gap-3">
          <div className="flex-1 min-w-0">
            <SheetTitle className="text-base leading-snug">{holding.instrument_name}</SheetTitle>
            <div className="flex items-center gap-2 mt-1 flex-wrap">
              {holding.isin && (
                <span className="text-xs text-muted-foreground font-mono">{holding.isin}</span>
              )}
              <span className={cn("text-xs px-1.5 py-0.5 rounded", color)}>{holding.instrument_type}</span>
              <span className="text-xs text-muted-foreground">{holding.account_name}</span>
            </div>
          </div>
        </div>

        {/* Mini summary */}
        <div className="grid grid-cols-3 gap-3 mt-3">
          <div>
            <p className="text-xs text-muted-foreground">Qty held</p>
            <p className="text-sm font-semibold tabular-nums">{formatQty(holding.quantity)}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Avg Cost</p>
            <p className="text-sm font-semibold tabular-nums">{formatINR(holding.avg_cost_paise)}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Invested</p>
            <p className="text-sm font-semibold tabular-nums">{formatINR(invested)}</p>
          </div>
          {holding.current_price_paise != null && (
            <>
              <div>
                <p className="text-xs text-muted-foreground">LTP</p>
                <p className="text-sm font-semibold tabular-nums">{formatINR(holding.current_price_paise)}</p>
              </div>
              <div>
                <p className="text-xs text-muted-foreground">Market Value</p>
                <p className="text-sm font-semibold tabular-nums">
                  {holding.current_value_paise != null ? formatINR(holding.current_value_paise) : "—"}
                </p>
              </div>
              {pnl != null && (
                <div>
                  <p className="text-xs text-muted-foreground">P&L</p>
                  <p className={cn("text-sm font-semibold tabular-nums", pnlPos ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400")}>
                    {pnlPos ? "+" : ""}{formatINR(pnl)}
                    {holding.unrealized_pnl_pct != null && (
                      <span className="text-xs font-normal ml-1">
                        ({pnlPos ? "+" : ""}{holding.unrealized_pnl_pct.toFixed(2)}%)
                      </span>
                    )}
                  </p>
                </div>
              )}
            </>
          )}
        </div>
      </SheetHeader>

      {/* Transaction table */}
      <div className="flex-1 overflow-auto">
        <div className="px-5 py-2.5 border-b shrink-0">
          <p className="text-xs font-medium text-muted-foreground">
            Transaction History · {transactions.length} record{transactions.length !== 1 ? "s" : ""}
          </p>
        </div>

        {loading ? (
          <div className="flex items-center justify-center py-12 text-sm text-muted-foreground">Loading…</div>
        ) : transactions.length === 0 ? (
          <div className="flex items-center justify-center py-12 text-sm text-muted-foreground">No transactions found.</div>
        ) : (
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 bg-background z-10">
              <tr className="border-b text-muted-foreground">
                <th className="py-2 px-3 text-left font-medium">Date</th>
                <th className="py-2 px-3 text-left font-medium">Time</th>
                <th className="py-2 px-3 text-left font-medium">Type</th>
                <th className="py-2 px-3 text-right font-medium">Qty</th>
                <th className="py-2 px-3 text-right font-medium">Price</th>
                <th className="py-2 px-3 text-right font-medium">Brokerage</th>
                <th className="py-2 px-3 text-right font-medium">STT+Charges</th>
                <th className="py-2 px-3 text-right font-medium">Net Value</th>
                <th className="py-2 px-3 text-left font-medium">Ref</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {transactions.map((txn) => (
                <TxnRow key={txn.txn_id} txn={txn} onOpenBatch={onOpenBatch} />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

function TxnRow({ txn, onOpenBatch }: { txn: Transaction; onOpenBatch: (id: number) => void }) {
  const isBuy = ["BUY","SIP","OPENING_BALANCE","BONUS","MERGER_IN","SWITCH_IN","TRANSFER_IN"].includes(txn.txn_type);
  const color = TXN_TYPE_COLORS[txn.txn_type] ?? "text-foreground";
  const sttAndOther = txn.stt_paise + txn.other_charges_paise;
  const clickable = txn.batch_id != null;

  return (
    <tr
      className={cn(
        "group",
        txn.flag && !txn.flag_dismissed ? "bg-red-50/50 dark:bg-red-950/20" : "hover:bg-muted/20",
        clickable ? "cursor-pointer" : "",
      )}
      onClick={clickable ? () => onOpenBatch(txn.batch_id!) : undefined}
    >
      <td className="py-2 px-3 text-muted-foreground whitespace-nowrap">{formatDate(txn.trade_date)}</td>
      <td className="py-2 px-3 text-muted-foreground tabular-nums whitespace-nowrap font-mono">
        {txn.txn_time ? txn.txn_time.slice(0, 5) : "—"}
      </td>
      <td className="py-2 px-3">
        <div className="flex items-center gap-1.5">
          <span className={cn("font-semibold", color)}>{txn.txn_type}</span>
          {txn.flag && !txn.flag_dismissed && (
            <Badge variant="destructive" className="text-xs py-0 h-4 px-1">{txn.flag}</Badge>
          )}
          {clickable && (
            <FileText className="size-3 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity shrink-0" />
          )}
        </div>
      </td>
      <td className="py-2 px-3 text-right tabular-nums">{formatQty(txn.quantity)}</td>
      <td className="py-2 px-3 text-right tabular-nums">{formatINR(txn.price_paise)}</td>
      <td className="py-2 px-3 text-right tabular-nums text-muted-foreground">
        {txn.brokerage_paise > 0 ? formatINR(txn.brokerage_paise) : "—"}
      </td>
      <td className="py-2 px-3 text-right tabular-nums text-muted-foreground">
        {sttAndOther > 0 ? formatINR(sttAndOther) : "—"}
      </td>
      <td className={cn("py-2 px-3 text-right tabular-nums font-semibold whitespace-nowrap", isBuy ? "text-red-600 dark:text-red-400" : "text-green-600 dark:text-green-400")}>
        {isBuy ? "−" : "+"}{formatINR(Math.abs(txn.total_value_paise))}
      </td>
      <td className="py-2 px-3 font-mono text-muted-foreground text-xs max-w-[100px] truncate">
        {txn.broker_ref ?? "—"}
      </td>
    </tr>
  );
}

// ─── MultiSelectPopover ───────────────────────────────────────────────────────

function MultiSelectPopover<T extends string | number>({
  label, options, selected, onChange, disabled,
}: {
  label: string;
  options: { value: T; label: string }[];
  selected: T[];
  onChange: (values: T[]) => void;
  disabled?: boolean;
}) {
  const toggle = (value: T) => {
    if (selected.includes(value)) {
      onChange(selected.filter(v => v !== value));
    } else {
      onChange([...selected, value]);
    }
  };

  const displayLabel = selected.length === 0
    ? `All ${label}s`
    : selected.length === 1
      ? (options.find(o => o.value === selected[0])?.label ?? label)
      : `${selected.length} ${label}s`;

  return (
    <Popover>
      <PopoverTrigger
        className={cn(
          "inline-flex items-center gap-1.5 h-8 px-3 rounded-md border text-xs font-normal transition-colors",
          "bg-background hover:bg-muted/50",
          selected.length > 0
            ? "border-primary text-primary"
            : "border-border text-foreground",
          disabled && "opacity-50 cursor-not-allowed pointer-events-none"
        )}
      >
        {displayLabel}
        <ChevronsUpDown className="size-3 opacity-50" />
      </PopoverTrigger>
      <PopoverContent className="w-52 p-1" align="start">
        {options.length === 0 ? (
          <p className="text-xs text-muted-foreground px-2 py-1.5">No options</p>
        ) : (
          options.map(opt => (
            <button
              key={String(opt.value)}
              className="w-full flex items-center gap-2 px-2 py-1.5 rounded text-sm hover:bg-muted transition-colors text-left"
              onClick={() => toggle(opt.value)}
            >
              <div className={cn(
                "size-4 rounded border flex items-center justify-center shrink-0",
                selected.includes(opt.value) ? "bg-primary border-primary text-primary-foreground" : "border-muted-foreground/40"
              )}>
                {selected.includes(opt.value) && <Check className="size-3" />}
              </div>
              <span className="truncate">{opt.label}</span>
            </button>
          ))
        )}
      </PopoverContent>
    </Popover>
  );
}

// ─── SummaryCard ─────────────────────────────────────────────────────────────

function SummaryCard({ label, value, note, valueClass }: {
  label: string; value: string; note?: string; valueClass?: string;
}) {
  return (
    <div className="flex flex-row items-center gap-2 min-w-0">
      <p className="text-xs text-muted-foreground whitespace-nowrap">{`${label}:`}</p>
      <p className={cn("text-sm font-semibold tabular-nums", valueClass)}>{value}</p>
      {note && <p className="text-xs text-muted-foreground">{note}</p>}
    </div>
  );
}

// ─── BatchView ────────────────────────────────────────────────────────────────
// Shown inside the sheet in place of the transaction list when a row is clicked.

function BatchView({ batch, loading, onBack }: {
  batch: ImportBatch | null;
  loading: boolean;
  onBack: () => void;
}) {
  const filePaths: string[] = (() => {
    if (!batch?.file_name) return [];
    try { return JSON.parse(batch.file_name) as string[]; } catch { return []; }
  })();

  const hasCharges = batch
    ? batch.stt_paise + batch.stamp_charges_paise + batch.gst_paise +
      batch.trans_charges_paise + batch.other_charges_paise + batch.total_payable_paise > 0
    : false;

  return (
    <>
      {/* Header with back button */}
      <SheetHeader className="px-5 pt-4 pb-4 border-b shrink-0">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" className="size-7 shrink-0" onClick={onBack}>
            <ChevronRight className="size-4 rotate-180" />
          </Button>
          <div className="flex-1 min-w-0">
            <SheetTitle className="text-sm font-mono leading-snug">
              {batch?.ref_no ?? "Contract Note"}
            </SheetTitle>
            {batch && (
              <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 mt-0.5 text-xs text-muted-foreground">
                {batch.broker && <span>{batch.broker}</span>}
                {batch.batch_trade_date && <span>{formatDate(batch.batch_trade_date)}</span>}
                <span className="opacity-60">{batch.source_type}</span>
                <span>Imported {new Date(batch.imported_at + "Z").toLocaleDateString()}</span>
              </div>
            )}
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
                  <span className="max-w-[160px] truncate">{fp.split("/").pop()}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </SheetHeader>

      <div className="flex-1 overflow-y-auto">
        {loading && (
          <div className="flex items-center justify-center py-16 text-sm text-muted-foreground">Loading…</div>
        )}

        {batch && (
          <div className="px-5 py-4 space-y-5">
            {/* Charges breakdown */}
            {hasCharges && (
              <div>
                <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-2">Contract Note Charges</p>
                <div className="rounded-lg border bg-muted/20 overflow-hidden">
                  <table className="w-full text-sm">
                    <tbody className="divide-y">
                      {batch.stt_paise > 0 && (
                        <tr>
                          <td className="py-2 px-4 text-muted-foreground">STT</td>
                          <td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.stt_paise)}</td>
                        </tr>
                      )}
                      {batch.stamp_charges_paise > 0 && (
                        <tr>
                          <td className="py-2 px-4 text-muted-foreground">Stamp Duty</td>
                          <td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.stamp_charges_paise)}</td>
                        </tr>
                      )}
                      {batch.gst_paise > 0 && (
                        <tr>
                          <td className="py-2 px-4 text-muted-foreground">GST</td>
                          <td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.gst_paise)}</td>
                        </tr>
                      )}
                      {batch.trans_charges_paise > 0 && (
                        <tr>
                          <td className="py-2 px-4 text-muted-foreground">Transaction Charges</td>
                          <td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.trans_charges_paise)}</td>
                        </tr>
                      )}
                      {batch.other_charges_paise > 0 && (
                        <tr>
                          <td className="py-2 px-4 text-muted-foreground">Other Charges</td>
                          <td className="py-2 px-4 text-right tabular-nums">{formatINR(batch.other_charges_paise)}</td>
                        </tr>
                      )}
                      {batch.total_payable_paise > 0 && (
                        <tr className="bg-muted/30 font-medium">
                          <td className="py-2.5 px-4">Net Payable</td>
                          <td className="py-2.5 px-4 text-right tabular-nums font-semibold">{formatINR(batch.total_payable_paise)}</td>
                        </tr>
                      )}
                    </tbody>
                  </table>
                </div>
              </div>
            )}

            {/* All trades in this batch */}
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
                      const color = TXN_TYPE_COLORS[t.txn_type] ?? "text-foreground";
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
          </div>
        )}
      </div>
    </>
  );
}
