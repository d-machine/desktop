import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useReactTable, getCoreRowModel, getSortedRowModel,
  flexRender, type ColumnDef, type SortingState,
} from "@tanstack/react-table";
import { ArrowUp, ArrowDown, ArrowUpDown } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { formatINR, formatDate } from "@/lib/format";
import { cn } from "@/lib/utils";

interface IncomeEvent {
  txn_id: number;
  instrument_id: number;
  instrument_name: string;
  isin?: string;
  asset_class: string;
  account_name: string;
  income_type: string;
  trade_date: string;
  amount_paise: number;
  notes?: string;
  fy: string;
}

interface IncomeSummary {
  fy: string;
  dividend_paise: number;
  interest_paise: number;
  total_paise: number;
}

interface IncomeReport {
  events: IncomeEvent[];
  summaries: IncomeSummary[];
  all_fys: string[];
}

const INCOME_TYPE_COLORS: Record<string, string> = {
  DIVIDEND: "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400",
  INTEREST: "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
};

const ASSET_CLASS_COLORS: Record<string, string> = {
  EQUITY:       "bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-400",
  MF:           "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400",
  FIXED_INCOME: "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
};

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

type IncomeTypeFilter = "ALL" | "DIVIDEND" | "INTEREST";

export function IncomePage() {
  const [report, setReport] = useState<IncomeReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedFY, setSelectedFY] = useState<string>("");
  const [typeFilter, setTypeFilter] = useState<IncomeTypeFilter>("ALL");
  const [sorting, setSorting] = useState<SortingState>([{ id: "trade_date", desc: true }]);

  const load = async (fy?: string) => {
    setLoading(true);
    try {
      const r = await invoke<IncomeReport>("get_income", {
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

  const filtered = useMemo(() => {
    if (!report) return [];
    if (typeFilter === "ALL") return report.events;
    return report.events.filter(e => e.income_type === typeFilter);
  }, [report, typeFilter]);

  const columns = useMemo<ColumnDef<IncomeEvent>[]>(() => [
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
            <span className={cn("text-xs px-1.5 py-0 rounded font-medium", INCOME_TYPE_COLORS[row.original.income_type])}>
              {row.original.income_type}
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
      size: 150,
    },
    {
      id: "trade_date",
      accessorKey: "trade_date",
      header: ({ column }) => <SortHeader column={column} label="Date" />,
      cell: ({ row }) => <span className="text-sm">{formatDate(row.original.trade_date)}</span>,
      size: 120,
    },
    {
      id: "amount_paise",
      accessorKey: "amount_paise",
      header: ({ column }) => <SortHeader column={column} label="Amount" right />,
      cell: ({ row }) => (
        <span className="text-sm tabular-nums text-right block font-medium text-green-600 dark:text-green-400">
          {formatINR(row.original.amount_paise)}
        </span>
      ),
      size: 130,
    },
    {
      id: "notes",
      accessorKey: "notes",
      header: "Notes",
      cell: ({ row }) => row.original.notes
        ? <span className="text-xs text-muted-foreground truncate max-w-[200px] block">{row.original.notes}</span>
        : null,
    },
  ], []);

  const table = useReactTable({
    data: filtered,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  const isEmpty = !loading && filtered.length === 0;
  const totalAmount = filtered.reduce((s, e) => s + e.amount_paise, 0);

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header + FY selector */}
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">Income</h1>
          <p className="text-sm text-muted-foreground">Dividends &amp; interest received</p>
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

      {/* Summary cards */}
      {summary && (
        <div className="grid grid-cols-3 gap-3">
          <SummaryCard label="Dividends" value={formatINR(summary.dividend_paise)} note="Taxed at slab rate" />
          <SummaryCard label="Interest" value={formatINR(summary.interest_paise)} note="Taxed at slab rate" />
          <SummaryCard label="Total Income" value={formatINR(summary.total_paise)} highlight />
        </div>
      )}

      {/* Type filter */}
      <div className="flex gap-1.5">
        {(["ALL", "DIVIDEND", "INTEREST"] as IncomeTypeFilter[]).map((t) => (
          <button
            key={t}
            onClick={() => setTypeFilter(t)}
            className={cn(
              "px-3 py-1 rounded-full text-xs font-medium border transition-colors",
              typeFilter === t
                ? "bg-primary text-primary-foreground border-primary"
                : "bg-background text-muted-foreground border-border hover:border-foreground/30"
            )}
          >
            {t === "ALL" ? "All" : t}
            {t !== "ALL" && report && (
              <span className="ml-1 opacity-60">
                ({report.events.filter(e => e.income_type === t).length})
              </span>
            )}
          </button>
        ))}
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
              ) : isEmpty ? (
                <tr>
                  <td colSpan={columns.length} className="px-3 py-12 text-center">
                    <p className="text-muted-foreground text-sm">No income events yet.</p>
                    <p className="text-xs text-muted-foreground mt-1">Add DIVIDEND or INTEREST transactions to track income here.</p>
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
            {!loading && !isEmpty && (
              <tfoot className="bg-muted/50 sticky bottom-0 border-t">
                <tr>
                  <td colSpan={3} className="px-3 py-2 text-xs font-medium text-muted-foreground">
                    {filtered.length} event{filtered.length !== 1 ? "s" : ""}
                  </td>
                  <td className="px-3 py-2 text-right">
                    <span className="text-sm tabular-nums font-semibold text-green-600 dark:text-green-400">
                      {formatINR(totalAmount)}
                    </span>
                  </td>
                  <td />
                </tr>
              </tfoot>
            )}
          </table>
        </div>
      </div>
    </div>
  );
}

function SummaryCard({ label, value, note, highlight }: {
  label: string; value: string; note?: string; highlight?: boolean;
}) {
  return (
    <Card>
      <CardContent className="pt-4 pb-4">
        <p className="text-xs text-muted-foreground">{label}</p>
        <p className={cn(
          "text-xl font-semibold mt-0.5 tabular-nums",
          highlight ? "text-green-600 dark:text-green-400" : ""
        )}>{value}</p>
        {note && <p className="text-xs text-muted-foreground mt-0.5">{note}</p>}
      </CardContent>
    </Card>
  );
}
