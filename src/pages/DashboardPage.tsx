import { useEffect, useState, useMemo } from "react";
import { apiPost } from "@/lib/api";
import { ArrowUpRight } from "lucide-react";
import { formatINR, formatDate } from "@/lib/format";
import { cn } from "@/lib/utils";
import { type Page } from "@/lib/nav";
import { TXN_TYPE_COLORS } from "@/lib/txn-types";

interface PortfolioSummary {
  total_invested_paise: number;
  current_value_paise?: number;
  unrealized_pnl_paise?: number;
  unrealized_pnl_pct?: number;
  holdings_count: number;
}

interface Holding {
  instrument_id: number;
  instrument_name: string;
  asset_class: string;
  total_cost_paise: number;
  current_value_paise?: number;
  unrealized_pnl_paise?: number;
}

interface Transaction {
  txn_id: number;
  instrument_name: string;
  txn_type: string;
  trade_date: string;
  quantity: number;
  effective_price_paise: number;
}

interface CapitalGainsSummary {
  fy: string;
  stcg_equity_paise: number;
  ltcg_equity_paise: number;
  stcg_debt_paise: number;
  ltcg_debt_paise: number;
  total_gain_paise: number;
}

interface CapitalGainsReport {
  summaries: CapitalGainsSummary[];
  all_fys: string[];
}

const DONUT_COLORS: Record<string, string> = {
  EQUITY:       "#3b82f6",
  MF:           "#a855f7",
  FIXED_INCOME: "#22c55e",
  INSURANCE:    "#14b8a6",
  DERIVATIVE:   "#f97316",
  COMMODITY:    "#eab308",
  REAL_ESTATE:  "#f43f5e",
  ALTERNATIVES: "#64748b",
};

const ASSET_CLASS_LABELS: Record<string, string> = {
  EQUITY:       "Equity",
  MF:           "Mutual Funds",
  FIXED_INCOME: "Fixed Income",
  INSURANCE:    "Insurance",
  DERIVATIVE:   "Derivatives",
  COMMODITY:    "Commodities",
  REAL_ESTATE:  "Real Estate",
  ALTERNATIVES: "Alternatives",
};

interface DashboardPageProps {
  onNavigate: (page: Page, instrumentId?: number) => void;
  personPortfolioIds: number[] | null;
  personAccountIds: number[] | null;
}

export function DashboardPage({ onNavigate, personPortfolioIds, personAccountIds }: DashboardPageProps) {
  const [summary, setSummary]       = useState<PortfolioSummary | null>(null);
  const [holdings, setHoldings]     = useState<Holding[]>([]);
  const [recentTxns, setRecentTxns] = useState<Transaction[]>([]);
  const [cgReport, setCgReport]     = useState<CapitalGainsReport | null>(null);
  const [loading, setLoading]       = useState(true);

  useEffect(() => {
    Promise.all([
      apiPost<PortfolioSummary>("/holdings/summary", { account_ids: null, portfolio_ids: personPortfolioIds, asset_classes: null }),
      apiPost<Holding[]>("/holdings", { account_ids: null, portfolio_ids: personPortfolioIds, asset_classes: null }),
      apiPost<Transaction[]>("/transactions/list", { limit: 10, account_ids: personAccountIds }),
      apiPost<CapitalGainsReport>("/reports/capital-gains", { fy: null, account_ids: personAccountIds }),
    ]).then(([s, h, t, cg]) => {
      setSummary(s);
      setHoldings(h);
      const sorted = [...t].sort((a, b) => b.trade_date.localeCompare(a.trade_date));
      setRecentTxns(sorted.slice(0, 5));
      setCgReport(cg);
    }).finally(() => setLoading(false));
  }, []);

  const top5 = useMemo(() => {
    return [...holdings]
      .sort((a, b) =>
        (b.current_value_paise ?? b.total_cost_paise) - (a.current_value_paise ?? a.total_cost_paise)
      )
      .slice(0, 5);
  }, [holdings]);

  const totalVal = useMemo(() =>
    holdings.reduce((s, h) => s + (h.current_value_paise ?? h.total_cost_paise), 0),
    [holdings]
  );

  const allocationData = useMemo(() => {
    const byClass: Record<string, number> = {};
    for (const h of holdings) {
      const val = h.current_value_paise ?? h.total_cost_paise;
      byClass[h.asset_class] = (byClass[h.asset_class] ?? 0) + val;
    }
    return Object.entries(byClass)
      .sort(([, a], [, b]) => b - a)
      .map(([cls, val]) => ({
        label: ASSET_CLASS_LABELS[cls] ?? cls,
        value: val,
        color: DONUT_COLORS[cls] ?? "#94a3b8",
      }));
  }, [holdings]);

  const cgSummary = cgReport?.summaries[0];
  const currentFY = cgReport?.all_fys[0];
  const pnl = summary?.unrealized_pnl_paise ?? 0;
  const pnlPct = summary?.unrealized_pnl_pct;

  if (loading) {
    return <div className="text-sm text-muted-foreground py-8 text-center">Loading…</div>;
  }

  return (
    <div className="flex flex-col gap-5">
      <h1 className="text-2xl font-semibold">Dashboard</h1>

      {/* Summary cards */}
      <div className="grid grid-cols-4 gap-3">
        <SummaryCard
          label="Current Value"
          value={summary?.current_value_paise != null ? formatINR(summary.current_value_paise) : "—"}
        />
        <SummaryCard
          label="Invested"
          value={formatINR(summary?.total_invested_paise ?? 0)}
        />
        <SummaryCard
          label="Unrealized P&L"
          value={summary?.unrealized_pnl_paise != null ? formatINR(Math.abs(pnl)) : "—"}
          prefix={summary?.unrealized_pnl_paise != null ? (pnl >= 0 ? "+" : "−") : undefined}
          positive={pnl >= 0}
          colored={summary?.unrealized_pnl_paise != null}
        />
        <SummaryCard
          label="Return"
          value={pnlPct != null ? `${Math.abs(pnlPct).toFixed(2)}%` : "—"}
          prefix={pnlPct != null ? (pnlPct >= 0 ? "+" : "−") : undefined}
          positive={pnlPct != null ? pnlPct >= 0 : true}
          colored={pnlPct != null}
        />
      </div>

      {/* Allocation + Top Holdings */}
      <div className="grid grid-cols-3 gap-4">
        <div className="border rounded-lg p-4 flex flex-col gap-4">
          <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Allocation</p>
          {allocationData.length > 0 ? (
            <div className="flex items-center gap-5">
              <DonutChart slices={allocationData} />
              <div className="flex flex-col gap-2 flex-1 min-w-0">
                {allocationData.map(d => {
                  const pct = totalVal > 0 ? (d.value / totalVal * 100).toFixed(1) : "0";
                  return (
                    <div key={d.label} className="flex items-center gap-1.5 min-w-0">
                      <div className="size-2 rounded-full shrink-0" style={{ background: d.color }} />
                      <span className="text-xs text-muted-foreground truncate">{d.label}</span>
                      <span className="text-xs font-medium ml-auto pl-1 tabular-nums">{pct}%</span>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">No holdings yet.</p>
          )}
        </div>

        <div className="col-span-2 border rounded-lg p-4 flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Top Holdings</p>
            <button
              onClick={() => onNavigate("holdings")}
              className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-0.5 transition-colors"
            >
              View all <ArrowUpRight className="size-3" />
            </button>
          </div>
          {top5.length > 0 ? (
            <table className="w-full">
              <thead>
                <tr className="text-xs text-muted-foreground border-b">
                  <th className="text-left pb-2 font-medium">Instrument</th>
                  <th className="text-right pb-2 font-medium">Value</th>
                  <th className="text-right pb-2 font-medium">P&L</th>
                  <th className="text-right pb-2 font-medium">Weight</th>
                </tr>
              </thead>
              <tbody className="divide-y">
                {top5.map(h => {
                  const val = h.current_value_paise ?? h.total_cost_paise;
                  const weight = totalVal > 0 ? (val / totalVal * 100).toFixed(1) : "0";
                  const pnl = h.unrealized_pnl_paise;
                  const pos = pnl == null ? true : pnl >= 0;
                  return (
                    <tr
                      key={h.instrument_id}
                      className="hover:bg-muted/30 cursor-pointer transition-colors group"
                      onClick={() => onNavigate("holdings", h.instrument_id)}
                    >
                      <td className="py-2 pr-3">
                        <span className="text-sm font-medium truncate block max-w-[160px] group-hover:text-primary transition-colors">
                          {h.instrument_name}
                        </span>
                      </td>
                      <td className="py-2 text-right tabular-nums text-sm">{formatINR(val)}</td>
                      <td className={cn(
                        "py-2 text-right tabular-nums text-sm",
                        pnl != null ? (pos ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400") : "text-muted-foreground"
                      )}>
                        {pnl != null ? `${pos ? "+" : "−"}${formatINR(Math.abs(pnl))}` : "—"}
                      </td>
                      <td className="py-2 text-right tabular-nums text-sm text-muted-foreground">{weight}%</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          ) : (
            <p className="text-sm text-muted-foreground">No holdings yet.</p>
          )}
        </div>
      </div>

      {/* Recent Transactions + Capital Gains */}
      <div className="grid grid-cols-2 gap-4">
        <div className="border rounded-lg p-4 flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">Recent Transactions</p>
            <button
              onClick={() => onNavigate("transactions")}
              className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-0.5 transition-colors"
            >
              View all <ArrowUpRight className="size-3" />
            </button>
          </div>
          {recentTxns.length > 0 ? (
            <div className="divide-y">
              {recentTxns.map(t => (
                <div key={t.txn_id} className="flex items-center gap-3 py-2">
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium truncate">{t.instrument_name}</p>
                    <p className="text-xs text-muted-foreground">{formatDate(t.trade_date)}</p>
                  </div>
                  <span className={cn("text-xs font-semibold shrink-0", TXN_TYPE_COLORS[t.txn_type] ?? "text-muted-foreground")}>
                    {t.txn_type}
                  </span>
                  <span className="text-sm tabular-nums shrink-0">{formatINR(Math.round(t.quantity * t.effective_price_paise))}</span>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">No transactions yet.</p>
          )}
        </div>

        <div className="border rounded-lg p-4 flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
              Capital Gains{currentFY ? ` · FY ${currentFY}` : ""}
            </p>
            <button
              onClick={() => onNavigate("reports")}
              className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-0.5 transition-colors"
            >
              View details <ArrowUpRight className="size-3" />
            </button>
          </div>
          {cgSummary ? (
            <div className="grid grid-cols-2 gap-x-4 gap-y-3">
              <CGItem label="STCG · Equity" paise={cgSummary.stcg_equity_paise} />
              <CGItem label="LTCG · Equity" paise={cgSummary.ltcg_equity_paise} />
              <CGItem label="STCG · Debt"   paise={cgSummary.stcg_debt_paise} />
              <CGItem label="LTCG · Debt"   paise={cgSummary.ltcg_debt_paise} />
              <div className="col-span-2 border-t pt-2.5 flex items-center justify-between">
                <span className="text-xs font-medium text-muted-foreground">Total Realized Gain</span>
                <span className={cn(
                  "text-sm font-semibold tabular-nums",
                  cgSummary.total_gain_paise >= 0 ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"
                )}>
                  {cgSummary.total_gain_paise >= 0 ? "+" : "−"}{formatINR(Math.abs(cgSummary.total_gain_paise))}
                </span>
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">No realized gains this FY.</p>
          )}
        </div>
      </div>
    </div>
  );
}

function SummaryCard({ label, value, prefix, positive, colored }: {
  label: string;
  value: string;
  prefix?: string;
  positive?: boolean;
  colored?: boolean;
}) {
  return (
    <div className="border rounded-lg px-4 py-3 flex flex-col gap-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className={cn(
        "text-lg font-semibold tabular-nums",
        colored ? (positive ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400") : ""
      )}>
        {prefix}{value}
      </p>
    </div>
  );
}

function CGItem({ label, paise }: { label: string; paise: number }) {
  const pos = paise >= 0;
  return (
    <div className="flex flex-col gap-0.5">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className={cn(
        "text-sm font-medium tabular-nums",
        paise !== 0 ? (pos ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400") : ""
      )}>
        {paise !== 0 ? (pos ? "+" : "−") : ""}{formatINR(Math.abs(paise))}
      </p>
    </div>
  );
}

function DonutChart({ slices }: { slices: Array<{ label: string; value: number; color: string }> }) {
  const total = slices.reduce((s, x) => s + x.value, 0);
  if (total === 0) return <div className="size-20 rounded-full bg-muted shrink-0" />;

  const r = 34, cx = 42, cy = 42, strokeWidth = 13;
  const C = 2 * Math.PI * r;
  let cumulative = 0;

  return (
    <svg width="84" height="84" viewBox="0 0 84 84" className="shrink-0">
      {slices.map(s => {
        const frac = s.value / total;
        const dashLen = frac * C;
        const offset = C / 4 - cumulative * C;
        cumulative += frac;
        return (
          <circle
            key={s.label}
            cx={cx} cy={cy} r={r}
            fill="none"
            stroke={s.color}
            strokeWidth={strokeWidth}
            strokeDasharray={`${dashLen} ${C}`}
            strokeDashoffset={offset}
          />
        );
      })}
    </svg>
  );
}
