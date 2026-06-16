import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { InstrumentSearch, InstrumentSummary } from "@/components/transactions/InstrumentSearch";
import { formatINR, formatQty } from "@/lib/format";

interface Holding {
  instrument_id: number;
  instrument_name: string;
  isin?: string;
  account_id: number;
  account_name: string;
  quantity: number;
  avg_cost_paise: number;
  total_cost_paise: number;
  is_pending: boolean;
}

interface SplitResult {
  ca_id: number;
  split_out_txn_id: number;
  split_in_txn_id: number;
}

interface Props {
  holding: Holding | null;
  onClose: () => void;
  onDone: () => void;
}

export function SplitDialog({ holding, onClose, onDone }: Props) {
  const open = holding !== null;

  const [qtyAfter, setQtyAfter]       = useState<string>("");
  const [toInstrument, setToInstrument] = useState<InstrumentSummary | undefined>(undefined);
  const [tradeDate, setTradeDate]     = useState<string>(() => new Date().toISOString().slice(0, 10));
  const [notes, setNotes]             = useState<string>("");
  const [submitting, setSubmitting]   = useState(false);
  const [error, setError]             = useState<string | null>(null);

  useEffect(() => {
    if (!holding) return;
    setQtyAfter("");
    setToInstrument(undefined);
    setTradeDate(new Date().toISOString().slice(0, 10));
    setNotes("");
    setError(null);
  }, [holding]);

  if (!holding) return null;

  const qtyAfterNum = parseFloat(qtyAfter);
  const ratio = !isNaN(qtyAfterNum) && qtyAfterNum > 0 && holding.quantity > 0
    ? qtyAfterNum / holding.quantity
    : null;
  const newAvgCost = ratio != null
    ? Math.round(holding.avg_cost_paise / ratio)
    : null;

  const handleSubmit = async () => {
    setError(null);

    if (!qtyAfter || isNaN(qtyAfterNum) || qtyAfterNum <= 0) {
      setError("Enter a valid new quantity.");
      return;
    }
    if (!toInstrument) {
      setError("Select the post-split instrument.");
      return;
    }
    if (!tradeDate) {
      setError("Enter the ex-date.");
      return;
    }

    const toInstrumentId   = toInstrument.instrument_id > 0 ? toInstrument.instrument_id : null;
    const toPending        = toInstrument.pending_instrument ? toInstrument.pending_instrument : null;

    if (toInstrumentId === null && toPending === null) {
      setError("Invalid post-split instrument.");
      return;
    }

    setSubmitting(true);
    try {
      await invoke<SplitResult>("create_split", {
        input: {
          account_id:            holding.account_id,
          from_instrument_id:    holding.instrument_id,
          to_instrument_id:      toInstrumentId,
          to_pending:            toPending,
          qty_before:            holding.quantity,
          qty_after:             qtyAfterNum,
          avg_cost_before_paise: holding.avg_cost_paise,
          trade_date:            tradeDate,
          notes:                 notes.trim() || null,
        },
      });
      onDone();
    } catch (e: unknown) {
      setError(String(e) ?? "Split failed.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) onClose(); }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Record Stock Split / ISIN Change</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* From instrument summary */}
          <div className="rounded-md bg-muted/50 px-3 py-2.5 text-sm space-y-0.5">
            <div className="font-medium">{holding.instrument_name}</div>
            <div className="text-xs text-muted-foreground">
              {holding.account_name}
              {holding.isin && <span className="ml-2 font-mono">{holding.isin}</span>}
              {holding.is_pending && (
                <span className="ml-2 px-1.5 py-0 rounded leading-5 bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400">
                  PENDING
                </span>
              )}
            </div>
            <div className="flex gap-4 mt-1.5 text-xs">
              <span>
                <span className="text-muted-foreground">Qty: </span>
                <span className="tabular-nums font-medium">{formatQty(holding.quantity)}</span>
              </span>
              <span>
                <span className="text-muted-foreground">Avg cost: </span>
                <span className="tabular-nums font-medium">{formatINR(holding.avg_cost_paise)}</span>
              </span>
              <span>
                <span className="text-muted-foreground">Total: </span>
                <span className="tabular-nums font-medium">{formatINR(holding.total_cost_paise)}</span>
              </span>
            </div>
          </div>

          {/* New quantity */}
          <div className="space-y-1.5">
            <Label>New Quantity (after split)</Label>
            <Input
              type="number"
              min="0"
              step="any"
              value={qtyAfter}
              onChange={e => setQtyAfter(e.target.value)}
              placeholder={`Currently: ${formatQty(holding.quantity)}`}
            />
            {ratio != null && (
              <p className="text-xs text-muted-foreground">
                Ratio <span className="tabular-nums font-medium">{ratio.toFixed(4)}×</span>
                {" · "}New avg cost <span className="tabular-nums font-medium">{formatINR(newAvgCost!)}</span>
                {" · "}Total preserved at <span className="tabular-nums font-medium">{formatINR(holding.total_cost_paise)}</span>
              </p>
            )}
          </div>

          {/* Post-split instrument */}
          <div className="space-y-1.5">
            <Label>Post-Split Instrument (new ISIN / symbol)</Label>
            <InstrumentSearch
              value={toInstrument}
              onChange={(v) => setToInstrument(v ?? undefined)}
              placeholder="Search by name, new ISIN or symbol…"
            />
            <p className="text-xs text-muted-foreground">
              Search the catalog or add manually if not yet listed.
            </p>
          </div>

          {/* Ex-date */}
          <div className="space-y-1.5">
            <Label>Ex-Date</Label>
            <Input
              type="date"
              value={tradeDate}
              onChange={e => setTradeDate(e.target.value)}
            />
          </div>

          {/* Notes */}
          <div className="space-y-1.5">
            <Label>Notes (optional)</Label>
            <Input
              value={notes}
              onChange={e => setNotes(e.target.value)}
              placeholder="e.g. 10:1 stock split, ISIN changed"
            />
          </div>

          {/* Cost basis preview */}
          {toInstrument && ratio != null && (
            <div className="rounded-md border px-3 py-2.5 text-sm space-y-1">
              <p className="text-xs font-medium text-muted-foreground mb-2">Cost basis after split</p>
              <div className="flex justify-between">
                <span className="text-muted-foreground">SPLIT OUT</span>
                <span className="font-mono text-xs">{holding.instrument_name} × {formatQty(holding.quantity)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">SPLIT IN</span>
                <span className="font-mono text-xs">{toInstrument.name} × {formatQty(qtyAfterNum)}</span>
              </div>
              <div className="flex justify-between border-t pt-1 mt-1">
                <span className="text-muted-foreground">Total cost preserved</span>
                <span className="tabular-nums font-semibold">{formatINR(holding.total_cost_paise)}</span>
              </div>
            </div>
          )}

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={submitting}>Cancel</Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting ? "Recording…" : "Record Split"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
