import { useState, useEffect } from "react";
import { apiGet, apiPost, apiPatch } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription,
} from "@/components/ui/sheet";
import { InstrumentSearch, type InstrumentSummary } from "@/components/transactions/InstrumentSearch";
import { PendingInstrumentForm, type PendingInstrumentSpec, type AssetClass } from "@/components/transactions/PendingInstrumentForm";

interface PendingRow {
  pending_id: number;
  name: string;
  type: string;
  metadata: string;
  txn_count: number;
  earliest_date: string;
  account_ids: string;
}

type RowMode = "idle" | "mapping" | "confirming" | "enriching";

interface PendingInstrumentManagerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onResolved: () => void;
}

export function PendingInstrumentManager({ open, onOpenChange, onResolved }: PendingInstrumentManagerProps) {
  const [rows, setRows]             = useState<PendingRow[]>([]);
  const [loading, setLoading]       = useState(false);
  const [rowMode, setRowMode]       = useState<Record<number, RowMode>>({});
  const [mapping, setMapping]       = useState<Record<number, InstrumentSummary | null>>({});
  const [saving, setSaving]         = useState<Record<number, boolean>>({});
  const [rowMsg, setRowMsg]         = useState<Record<number, string>>({});
  const [rowErr, setRowErr]         = useState<Record<number, string>>({});

  useEffect(() => {
    if (open) loadPending();
  }, [open]);

  async function loadPending() {
    setLoading(true);
    try {
      const data = await apiGet<PendingRow[]>("/instruments/pending");
      setRows(data);
      setRowMode({});
      setMapping({});
      setSaving({});
      setRowMsg({});
      setRowErr({});
    } finally {
      setLoading(false);
    }
  }

  function setMode(id: number, mode: RowMode) {
    setRowMode(prev => ({ ...prev, [id]: mode }));
    setRowErr(prev => ({ ...prev, [id]: "" }));
  }

  async function handleConfirmMap(row: PendingRow) {
    const target = mapping[row.pending_id];
    if (!target) return;
    setSaving(prev => ({ ...prev, [row.pending_id]: true }));
    setRowErr(prev => ({ ...prev, [row.pending_id]: "" }));
    try {
      const res = await apiPost<{ ok: boolean; migrated_txns: number }>(
        `/instruments/pending/${row.pending_id}/resolve`,
        { instrument_id: target.instrument_id },
      );
      setRows(prev => prev.filter(r => r.pending_id !== row.pending_id));
      setRowMsg(prev => ({ ...prev, [row.pending_id]: `${res.migrated_txns} transaction(s) remapped` }));
      onResolved();
    } catch (e: any) {
      setRowErr(prev => ({ ...prev, [row.pending_id]: e?.message ?? "Failed" }));
      setMode(row.pending_id, "mapping");
    } finally {
      setSaving(prev => ({ ...prev, [row.pending_id]: false }));
    }
  }

  async function handleSaveEnrich(row: PendingRow, spec: PendingInstrumentSpec) {
    setSaving(prev => ({ ...prev, [row.pending_id]: true }));
    setRowErr(prev => ({ ...prev, [row.pending_id]: "" }));
    try {
      await apiPatch(`/instruments/pending/${row.pending_id}`, { name: spec.name, metadata: spec.metadata });
      setRows(prev => prev.map(r => r.pending_id === row.pending_id
        ? { ...r, name: spec.name, metadata: JSON.stringify(spec.metadata) }
        : r
      ));
      setMode(row.pending_id, "idle");
      setRowMsg(prev => ({ ...prev, [row.pending_id]: "Details saved — will resolve on next price sync" }));
    } catch (e: any) {
      setRowErr(prev => ({ ...prev, [row.pending_id]: e?.message ?? "Failed" }));
    } finally {
      setSaving(prev => ({ ...prev, [row.pending_id]: false }));
    }
  }

  const typeBadgeColor: Record<string, string> = {
    EQUITY:  "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300",
    MF:      "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300",
    FUTSTK: "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-300",
    FUTIDX: "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-300",
    OPTSTK: "bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-300",
    OPTIDX: "bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-300",
    MCX:    "bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-300",
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="w-full sm:max-w-xl overflow-y-auto">
        <SheetHeader className="mb-4">
          <SheetTitle>Pending Instruments</SheetTitle>
          <SheetDescription>
            Map each unresolved instrument to an existing entry, or add its ISIN / symbol so it resolves automatically on next sync.
          </SheetDescription>
        </SheetHeader>

        {loading && (
          <div className="py-8 text-center text-sm text-muted-foreground">Loading…</div>
        )}

        {!loading && rows.length === 0 && (
          <div className="py-8 text-center text-sm text-muted-foreground">No pending instruments with transactions.</div>
        )}

        <div className="space-y-3">
          {rows.map(row => {
            const mode    = rowMode[row.pending_id] ?? "idle";
            const target  = mapping[row.pending_id] ?? null;
            const isSaving = saving[row.pending_id] ?? false;
            const msg     = rowMsg[row.pending_id] ?? "";
            const err     = rowErr[row.pending_id] ?? "";
            const typeClass = typeBadgeColor[row.type] ?? "bg-muted text-muted-foreground";

            return (
              <div key={row.pending_id} className="border rounded-lg p-3 space-y-2 bg-card">
                {/* Header */}
                <div className="flex items-start gap-2">
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate">{row.name}</div>
                    <div className="text-xs text-muted-foreground mt-0.5">
                      {row.txn_count} transaction{row.txn_count !== 1 ? "s" : ""}
                      {row.earliest_date && <span> · since {row.earliest_date}</span>}
                    </div>
                  </div>
                  <span className={`text-xs font-medium px-1.5 py-0.5 rounded shrink-0 ${typeClass}`}>
                    {row.type}
                  </span>
                </div>

                {/* Success message */}
                {msg && mode === "idle" && (
                  <div className="text-xs text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded px-2 py-1">
                    {msg}
                  </div>
                )}

                {/* Error */}
                {err && (
                  <div className="text-xs text-destructive bg-destructive/10 border border-destructive/20 rounded px-2 py-1">{err}</div>
                )}

                {/* Idle: action buttons */}
                {mode === "idle" && (
                  <div className="flex gap-2">
                    <Button size="sm" variant="outline" className="flex-1" onClick={() => setMode(row.pending_id, "mapping")}>
                      Map to existing
                    </Button>
                    <Button size="sm" variant="outline" className="flex-1" onClick={() => setMode(row.pending_id, "enriching")}>
                      Add details
                    </Button>
                  </div>
                )}

                {/* Mapping: search for resolved instrument */}
                {mode === "mapping" && (
                  <div className="space-y-2">
                    <div className="text-xs text-muted-foreground">Search for the matching instrument in the database:</div>
                    <InstrumentSearch
                      value={target ?? undefined}
                      onChange={inst => {
                        // Only accept resolved instruments
                        if (inst.instrument_id > 0) {
                          setMapping(prev => ({ ...prev, [row.pending_id]: inst }));
                          setMode(row.pending_id, "confirming");
                        }
                      }}
                      placeholder="Search name, ISIN or symbol…"
                      resolvedOnly
                    />
                    <Button size="sm" variant="ghost" onClick={() => setMode(row.pending_id, "idle")} className="w-full">
                      Cancel
                    </Button>
                  </div>
                )}

                {/* Confirming: show selected target and confirm button */}
                {mode === "confirming" && target && (
                  <div className="space-y-2">
                    <div className="text-xs text-muted-foreground rounded border px-3 py-2 bg-muted/30">
                      Map <span className="font-medium text-foreground">{row.txn_count} transaction{row.txn_count !== 1 ? "s" : ""}</span> from{" "}
                      <span className="font-medium text-foreground">"{row.name}"</span> →{" "}
                      <span className="font-medium text-foreground">"{target.name}"</span>
                      {target.isin && <span className="text-muted-foreground"> ({target.isin})</span>}?
                    </div>
                    <div className="flex gap-2">
                      <Button size="sm" variant="outline" onClick={() => setMode(row.pending_id, "mapping")} className="flex-1" disabled={isSaving}>
                        Back
                      </Button>
                      <Button size="sm" onClick={() => handleConfirmMap(row)} className="flex-1" disabled={isSaving}>
                        {isSaving ? "Remapping…" : "Confirm map"}
                      </Button>
                    </div>
                  </div>
                )}

                {/* Enriching: add ISIN / symbol details */}
                {mode === "enriching" && (
                  <div className="space-y-2">
                    <PendingInstrumentForm
                      initialValues={{
                        name: row.name,
                        type: (row.type as AssetClass) ?? "EQUITY",
                        fields: (() => {
                          try { return JSON.parse(row.metadata) as Record<string, string>; }
                          catch { return { name: row.name }; }
                        })(),
                      }}
                      onConfirm={(spec) => handleSaveEnrich(row, spec)}
                      onCancel={() => setMode(row.pending_id, "idle")}
                      allowSkip={false}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </SheetContent>
    </Sheet>
  );
}
