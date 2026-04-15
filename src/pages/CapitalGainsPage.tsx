import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useReactTable, getCoreRowModel, getSortedRowModel,
  flexRender, type ColumnDef, type SortingState,
} from "@tanstack/react-table";
import { ArrowUp, ArrowDown, ArrowUpDown } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { formatINR, formatDate, formatQty } from "@/lib/format";
import { cn } from "@/lib/utils";

interface CapitalGainLot {
  instrument_id: number;
  instrument_name: string;
  isin?: string;
  asset_class: string;
  tax_category: string;
  account_name: string;
  buy_date: string;
  sell_date: string;
  quantity: number;
  buy_price_paise: number;
  sell_price_paise: number;
  cost_paise: number;
  proceeds_paise: number;
  gain_paise: number;
  holding_days: number;
  gain_type: string;
  fy: string;
}

interface CapitalGainsSummary {
  fy: string;
  stcg_equity_paise: number;
  ltcg_equity_paise: number;
  stcg_debt_paise: number;
  ltcg_debt_paise: number;
  speculative_paise: number;
  non_speculative_paise: number;
  total_gain_paise: number;
}

interface CapitalGainsReport {
  lots: CapitalGainLot[];
  summaries: CapitalGainsSummary[];
  all_fys: string[];
}

const GAIN_TYPE_COLORS: Record<string, string> = {
  LTCG:              "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
  STCG:              "bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400",
  SPECULATIVE:       "bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400",
  NON_SPECULATIVE:   "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400",
};

function GainBadge({ type }: { type: string }) {
  return (
    <span className={cn("text-xs px-1.5 py-0 rounded font-medium", GAIN_TYPE_COLORS[type] ?? "")}>
      {type.replace("_", " ")}
    </span>
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

function GainValue({ paise }: { paise: number }) {
  const positive = paise >= 0;
  return (
    <span className={cn(
      "text-sm tabular-nums font-medium",
      positive ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"
    )}>
      {positive ? "+" : ""}{formatINR(paise)}
    </span>
  );
}

export function CapitalGainsPage() {
  const [report, setReport] = useState<CapitalGainsReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedFY, setSelectedFY] = useState<string>("");
  const [sorting, setSorting] = useState<SortingState>([{ id: "sell_date", desc: true }]);

  const load = async (fy?: string) => {
    setLoading(true);
    try {
      const r = await invoke<CapitalGainsReport>("get_capital_gains", {
        fy: fy || null,
        accountIds: null,
      });
      setReport(r);
      if (!fy && r.all_fys.length > 0 && !selectedFY) {
        setSelectedFY(r.all_fys[0]);
      }
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, []);

  const handleFYChange = (fy: string) => {
    setSelectedFY(fy);
    load(fy);
  };

  const summary = useMemo(
    () => report?.summaries.find(s => s.fy === selectedFY) ?? report?.summaries[0],
    [report, selectedFY]
  );

  const columns = useMemo<ColumnDef<CapitalGainLot>[]>(() => [
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
            <GainBadge type={row.original.gain_type} />
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
      id: "buy_date",
      accessorKey: "buy_date",
      header: ({ column }) => <SortHeader column={column} label="Buy Date" />,
      cell: ({ row }) => <span className="text-sm">{formatDate(row.original.buy_date)}</span>,
      size: 110,
    },
    {
      id: "sell_date",
      accessorKey: "sell_date",
      header: ({ column }) => <SortHeader column={column} label="Sell Date" />,
      cell: ({ row }) => <span className="text-sm">{formatDate(row.original.sell_date)}</span>,
      size: 110,
    },
    {
      id: "holding_days",
      accessorKey: "holding_days",
      header: ({ column }) => <SortHeader column={column} label="Days" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block">{row.original.holding_days.toLocaleString("en-IN")}</span>
      ),
      size: 70,
    },
    {
      id: "quantity",
      accessorKey: "quantity",
      header: ({ column }) => <SortHeader column={column} label="Qty" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block">{formatQty(row.original.quantity)}</span>
      ),
      size: 80,
    },
    {
      id: "cost_paise",
      accessorKey: "cost_paise",
      header: ({ column }) => <SortHeader column={column} label="Cost" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block">{formatINR(row.original.cost_paise)}</span>
      ),
      size: 120,
    },
    {
      id: "proceeds_paise",
      accessorKey: "proceeds_paise",
      header: ({ column }) => <SortHeader column={column} label="Proceeds" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block">{formatINR(row.original.proceeds_paise)}</span>
      ),
      size: 120,
    },
    {
      id: "gain_paise",
      accessorKey: "gain_paise",
      header: ({ column }) => <SortHeader column={column} label="Gain / Loss" right />,
      cell: ({ row }) => (
        <div className="text-right">
          <GainValue paise={row.original.gain_paise} />
        </div>
      ),
      size: 130,
    },
  ], []);

  const table = useReactTable({
    data: report?.lots ?? [],
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  const isEmpty = !loading && (report?.lots.length ?? 0) === 0;

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header + FY selector */}
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">Capital Gains</h1>
          <p className="text-sm text-muted-foreground">FIFO-matched realised gains for ITR filing</p>
        </div>
        {(report?.all_fys.length ?? 0) > 0 && (
          <div className="flex gap-1.5 flex-wrap justify-end">
            {report!.all_fys.map((fy) => (
              <button
                key={fy}
                onClick={() => handleFYChange(fy)}
                className={cn(
                  "px-3 py-1 rounded-full text-xs font-medium border transition-colors",
                  selectedFY === fy
                    ? "bg-primary text-primary-foreground border-primary"
                    : "bg-background text-muted-foreground border-border hover:border-foreground/30"
                )}
              >
                FY {fy}
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Summary — 3 tax buckets */}
      {summary && (() => {
        const asPerSlab =
          summary.speculative_paise +
          summary.non_speculative_paise +
          summary.stcg_debt_paise +
          summary.ltcg_debt_paise;
        return (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <SummaryCard
              label="Short Term Capital Gains"
              sublabel="Equity held < 1 year"
              value={formatINR(summary.stcg_equity_paise)}
              valueClass={colorClass(summary.stcg_equity_paise)}
            />
            <SummaryCard
              label="Long Term Capital Gains"
              sublabel="Equity held ≥ 1 year"
              value={formatINR(summary.ltcg_equity_paise)}
              valueClass={colorClass(summary.ltcg_equity_paise)}
            />
            <SummaryCard
              label="As Per Slab"
              sublabel="Intraday · F&O · Debt"
              value={formatINR(asPerSlab)}
              valueClass={colorClass(asPerSlab)}
            />
          </div>
        );
      })()}

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
              ) : isEmpty ? (
                <tr>
                  <td colSpan={columns.length} className="px-3 py-12 text-center">
                    <p className="text-muted-foreground text-sm">No realised gains yet.</p>
                    <p className="text-xs text-muted-foreground mt-1">Add sell transactions to see capital gains here.</p>
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
            {/* Totals footer */}
            {!loading && !isEmpty && report && (
              <tfoot className="bg-muted/50 sticky bottom-0 border-t">
                <tr>
                  <td colSpan={6} className="px-3 py-2 text-xs font-medium text-muted-foreground">
                    {report.lots.length} matched lot{report.lots.length !== 1 ? "s" : ""}
                  </td>
                  <td className="px-3 py-2 text-right text-sm tabular-nums font-medium">
                    {formatINR(report.lots.reduce((s, l) => s + l.cost_paise, 0))}
                  </td>
                  <td className="px-3 py-2 text-right text-sm tabular-nums font-medium">
                    {formatINR(report.lots.reduce((s, l) => s + l.proceeds_paise, 0))}
                  </td>
                  <td className="px-3 py-2 text-right">
                    <GainValue paise={report.lots.reduce((s, l) => s + l.gain_paise, 0)} />
                  </td>
                </tr>
              </tfoot>
            )}
          </table>
        </div>
      </div>
    </div>
  );
}

function colorClass(paise: number) {
  if (paise === 0) return "";
  return paise > 0 ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400";
}

function SummaryCard({ label, sublabel, value, valueClass }: {
  label: string; sublabel?: string; value: string; valueClass?: string;
}) {
  return (
    <Card>
      <CardContent className="pt-4 pb-4">
        <p className="text-sm font-medium">{label}</p>
        {sublabel && <p className="text-xs text-muted-foreground mt-0.5">{sublabel}</p>}
        <p className={cn("text-2xl font-semibold mt-2 tabular-nums", valueClass)}>{value}</p>
      </CardContent>
    </Card>
  );
}
